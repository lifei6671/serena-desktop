use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::{DNSClass, LowerName, Name, RData, RecordType},
};
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    time::Duration,
};

const FALLBACK_TIMEOUT: Duration = Duration::from_secs(4);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1250);
const MAX_RESPONSE_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy)]
struct Provider {
    endpoint: &'static str,
}

const PROVIDERS: [Provider; 3] = [
    Provider {
        endpoint: "https://doh.pub/dns-query",
    },
    Provider {
        endpoint: "https://dns.alidns.com/dns-query",
    },
    Provider {
        endpoint: "https://cloudflare-dns.com/dns-query",
    },
];

pub(super) async fn resolve(
    host: &str,
    is_disallowed: fn(IpAddr) -> bool,
) -> std::result::Result<Vec<IpAddr>, ()> {
    tokio::time::timeout(FALLBACK_TIMEOUT, resolve_inner(host, is_disallowed))
        .await
        .map_err(|_| ())?
}

async fn resolve_inner(
    host: &str,
    is_disallowed: fn(IpAddr) -> bool,
) -> std::result::Result<Vec<IpAddr>, ()> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(REQUEST_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| ())?;
    for provider in PROVIDERS {
        if let Ok(addresses) = resolve_provider(&client, provider, host, is_disallowed).await {
            return Ok(addresses);
        }
    }
    Err(())
}

async fn resolve_provider(
    client: &reqwest::Client,
    provider: Provider,
    host: &str,
    is_disallowed: fn(IpAddr) -> bool,
) -> std::result::Result<Vec<IpAddr>, ()> {
    let (a, aaaa) = tokio::join!(
        query(client, provider, host, RecordType::A, is_disallowed),
        query(client, provider, host, RecordType::AAAA, is_disallowed),
    );
    let mut addresses = a?;
    addresses.extend(aaaa?);
    if addresses.is_empty() {
        return Err(());
    }
    Ok(addresses)
}

async fn query(
    client: &reqwest::Client,
    provider: Provider,
    host: &str,
    record_type: RecordType,
    is_disallowed: fn(IpAddr) -> bool,
) -> std::result::Result<Vec<IpAddr>, ()> {
    let request = build_query(host, record_type)?;
    let response = client
        .post(provider.endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/dns-message")
        .header(reqwest::header::ACCEPT, "application/dns-message")
        .body(request.to_vec().map_err(|_| ())?)
        .send()
        .await
        .map_err(|_| ())?;
    if response.status() != reqwest::StatusCode::OK
        || !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(is_doh_wire_media_type)
        || response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(());
    }
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_response(&request, &bytes, is_disallowed)
}

fn build_query(host: &str, record_type: RecordType) -> std::result::Result<Message, ()> {
    let mut name = Name::from_ascii(host).map_err(|_| ())?;
    name.set_fqdn(true);
    let mut request = Message::query();
    request.metadata.recursion_desired = true;
    request.add_query(Query::query(name, record_type));
    Ok(request)
}

fn is_doh_wire_media_type(value: &str) -> bool {
    let mut parts = value.splitn(2, ';');
    if !parts.next().is_some_and(|media_type| {
        media_type
            .trim()
            .eq_ignore_ascii_case("application/dns-message")
    }) {
        return false;
    }
    parts.next().is_none_or(media_type_parameters_are_valid)
}

fn media_type_parameters_are_valid(mut parameters: &str) -> bool {
    loop {
        parameters = parameters.trim_start_matches([' ', '\t']);
        let name_len = parameters
            .bytes()
            .take_while(|byte| is_http_token_byte(*byte))
            .count();
        if name_len == 0 {
            return false;
        }
        parameters = parameters[name_len..].trim_start_matches([' ', '\t']);
        let Some(value) = parameters.strip_prefix('=') else {
            return false;
        };
        parameters = value.trim_start_matches([' ', '\t']);
        if let Some(quoted) = parameters.strip_prefix('"') {
            let mut escaped = false;
            let mut end = None;
            for (index, byte) in quoted.bytes().enumerate() {
                if matches!(byte, b'\r' | b'\n') {
                    return false;
                }
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    end = Some(index + 1);
                    break;
                }
            }
            let Some(end) = end else {
                return false;
            };
            parameters = quoted[end..].trim_start_matches([' ', '\t']);
        } else {
            let value_len = parameters
                .bytes()
                .take_while(|byte| is_http_token_byte(*byte))
                .count();
            if value_len == 0 {
                return false;
            }
            parameters = parameters[value_len..].trim_start_matches([' ', '\t']);
        }
        if parameters.is_empty() {
            return true;
        }
        let Some(remaining) = parameters.strip_prefix(';') else {
            return false;
        };
        parameters = remaining;
    }
}

fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn parse_response(
    request: &Message,
    bytes: &[u8],
    is_disallowed: fn(IpAddr) -> bool,
) -> std::result::Result<Vec<IpAddr>, ()> {
    let response = Message::from_vec(bytes).map_err(|_| ())?;
    let request_question = request.queries.first().ok_or(())?;
    if request.queries.len() != 1
        || response.metadata.id != request.metadata.id
        || response.metadata.message_type != MessageType::Response
        || response.metadata.op_code != OpCode::Query
        || response.metadata.response_code != ResponseCode::NoError
        || response.metadata.truncation
        || !question_matches(&response.queries, request_question)
    {
        return Err(());
    }

    let mut allowed_owners = HashSet::from([LowerName::new(request_question.name())]);
    let mut cname_targets = HashMap::new();
    for _ in 0..response.answers.len() {
        let mut added_owner = false;
        for answer in &response.answers {
            let RData::CNAME(target) = &answer.data else {
                continue;
            };
            let owner = LowerName::new(&answer.name);
            if !allowed_owners.contains(&owner) {
                continue;
            }
            if answer.dns_class != DNSClass::IN {
                return Err(());
            }
            let target = LowerName::new(target);
            if let Some(existing) = cname_targets.get(&owner) {
                if existing != &target {
                    return Err(());
                }
                continue;
            }
            if allowed_owners.contains(&target) {
                return Err(());
            }
            cname_targets.insert(owner, target.clone());
            allowed_owners.insert(target);
            added_owner = true;
        }
        if !added_owner {
            break;
        }
    }

    let mut addresses = Vec::new();
    for answer in &response.answers {
        let owner = LowerName::new(&answer.name);
        if !allowed_owners.contains(&owner) {
            continue;
        }
        let ip = match &answer.data {
            RData::A(address) => IpAddr::V4((*address).into()),
            RData::AAAA(address) => IpAddr::V6((*address).into()),
            _ => continue,
        };
        if answer.dns_class != DNSClass::IN
            || answer.record_type() != request_question.query_type()
            || is_disallowed(ip)
        {
            return Err(());
        }
        addresses.push(ip);
    }
    Ok(addresses)
}

fn question_matches(questions: &[Query], request_question: &Query) -> bool {
    questions.len() == 1
        && questions[0].query_type() == request_question.query_type()
        && questions[0].query_class() == DNSClass::IN
        && LowerName::new(questions[0].name()) == LowerName::new(request_question.name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::rr::{Record, rdata::CNAME};
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn name(value: &str) -> Name {
        let mut name = Name::from_ascii(value).unwrap();
        name.set_fqdn(true);
        name
    }
    fn request(record_type: RecordType) -> Message {
        build_query("example.com", record_type).unwrap()
    }
    fn response_for(request: &Message) -> Message {
        let mut response = Message::response(request.metadata.id, OpCode::Query);
        response.add_query(request.queries[0].clone());
        response
    }
    fn wire(message: &Message) -> Vec<u8> {
        message.to_vec().unwrap()
    }
    fn a(owner: &str, address: Ipv4Addr) -> Record {
        Record::from_rdata(name(owner), 60, RData::A(address.into()))
    }
    fn aaaa(owner: &str, address: Ipv6Addr) -> Record {
        Record::from_rdata(name(owner), 60, RData::AAAA(address.into()))
    }
    fn cname(owner: &str, target: &str) -> Record {
        Record::from_rdata(name(owner), 60, RData::CNAME(CNAME(name(target))))
    }
    fn parse(request: &Message, response: &Message) -> Vec<IpAddr> {
        parse_response(request, &wire(response), |_| false).unwrap()
    }

    #[test]
    fn request_builder_emits_one_in_question_with_rd_and_correlatable_id() {
        for record_type in [RecordType::A, RecordType::AAAA] {
            let request = request(record_type);
            let decoded = Message::from_vec(&wire(&request)).unwrap();
            assert_eq!(decoded.metadata.id, request.metadata.id);
            assert_eq!(decoded.metadata.message_type, MessageType::Query);
            assert!(decoded.metadata.recursion_desired);
            assert_eq!(decoded.queries.len(), 1);
            assert_eq!(decoded.queries[0].query_type(), record_type);
            assert_eq!(decoded.queries[0].query_class(), DNSClass::IN);
            assert_eq!(
                LowerName::new(decoded.queries[0].name()),
                LowerName::new(&name("example.com"))
            );
        }
    }

    #[test]
    fn accepts_valid_direct_a_and_aaaa_responses() {
        let a_request = request(RecordType::A);
        let mut a_response = response_for(&a_request);
        a_response.add_answer(a("example.com", Ipv4Addr::new(93, 184, 216, 34)));
        assert_eq!(
            parse(&a_request, &a_response),
            vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]
        );
        let aaaa_request = build_query("example.com", RecordType::AAAA).unwrap();
        let mut aaaa_response = response_for(&aaaa_request);
        aaaa_response.add_answer(aaaa(
            "example.com",
            "2606:2800:220:1:248:1893:25c8:1946".parse().unwrap(),
        ));
        assert_eq!(
            parse(&aaaa_request, &aaaa_response),
            vec![IpAddr::V6(
                "2606:2800:220:1:248:1893:25c8:1946".parse().unwrap()
            )]
        );
    }

    #[test]
    fn rejects_invalid_response_headers_and_questions() {
        let request = request(RecordType::A);
        let mut id_mismatch = response_for(&request);
        id_mismatch.metadata.id = request.metadata.id.wrapping_add(1);
        let query_instead_of_response = request.clone();
        let mut wrong_opcode = response_for(&request);
        wrong_opcode.metadata.op_code = OpCode::Status;
        let mut non_noerror = response_for(&request);
        non_noerror.metadata.response_code = ResponseCode::ServFail;
        let mut truncated = response_for(&request);
        truncated.metadata.truncation = true;
        let mut wrong_question = response_for(&request);
        wrong_question.queries[0] = Query::query(name("other.example"), RecordType::A);
        let missing_question = Message::response(request.metadata.id, OpCode::Query);
        for response in [
            id_mismatch,
            query_instead_of_response,
            wrong_opcode,
            non_noerror,
            truncated,
            wrong_question,
            missing_question,
        ] {
            assert!(parse_response(&request, &wire(&response), |_| false).is_err());
        }
    }

    #[test]
    fn accepts_only_dns_message_media_type_with_valid_optional_parameters() {
        for value in [
            "application/dns-message",
            "Application/DNS-Message; charset=utf-8",
            "application/dns-message; profile=\"resolver response\"",
            "application/dns-message; profile=\"resolver; response\"",
        ] {
            assert!(is_doh_wire_media_type(value));
        }
        for value in [
            "application/dns-json",
            "application/json",
            "application/dns-message; charset",
            "application/dns-message; charset=",
        ] {
            assert!(!is_doh_wire_media_type(value));
        }
    }

    #[test]
    fn ignores_unrelated_special_addresses_and_cname_records() {
        let request = request(RecordType::A);
        let mut response = response_for(&request);
        response.add_answer(a("unrelated.example", Ipv4Addr::new(10, 0, 0, 1)));
        response.add_answer(cname("unrelated.example", "alias.example"));
        response.add_answer(a("alias.example", Ipv4Addr::new(192, 0, 2, 1)));
        response.add_answer(a("example.com", Ipv4Addr::new(93, 184, 216, 34)));
        assert_eq!(
            parse_response(
                &request,
                &wire(&response),
                crate::oauth::cimd::is_disallowed_ip
            ),
            Ok(vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))])
        );
    }

    #[test]
    fn accepts_direct_one_hop_and_multi_hop_cname_chains() {
        let request = request(RecordType::A);
        let mut direct = response_for(&request);
        direct.add_answer(a("example.com", Ipv4Addr::new(93, 184, 216, 34)));
        assert_eq!(
            parse(&request, &direct),
            vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]
        );
        let mut one_hop = response_for(&request);
        one_hop.add_answer(cname("example.com", "alias.example"));
        one_hop.add_answer(a("alias.example", Ipv4Addr::new(93, 184, 216, 34)));
        assert_eq!(
            parse(&request, &one_hop),
            vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]
        );
        let mut multi_hop = response_for(&request);
        multi_hop.add_answer(cname("example.com", "alias.example"));
        multi_hop.add_answer(cname("alias.example", "final.example"));
        multi_hop.add_answer(a("final.example", Ipv4Addr::new(93, 184, 216, 34)));
        assert_eq!(
            parse(&request, &multi_hop),
            vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]
        );
    }

    #[test]
    fn rejects_conflicting_or_looping_relevant_cnames_and_disallowed_or_wrong_family_addresses() {
        let request = request(RecordType::A);
        let mut conflicting = response_for(&request);
        conflicting.add_answer(cname("example.com", "one.example"));
        conflicting.add_answer(cname("example.com", "two.example"));
        let mut looped = response_for(&request);
        looped.add_answer(cname("example.com", "alias.example"));
        looped.add_answer(cname("alias.example", "example.com"));
        let mut disallowed = response_for(&request);
        disallowed.add_answer(a("example.com", Ipv4Addr::new(10, 0, 0, 1)));
        let mut wrong_family = response_for(&request);
        wrong_family.add_answer(aaaa("example.com", "2001:4860:4860::8888".parse().unwrap()));
        for response in [conflicting, looped] {
            assert!(parse_response(&request, &wire(&response), |_| false).is_err());
        }
        assert!(
            parse_response(
                &request,
                &wire(&disallowed),
                crate::oauth::cimd::is_disallowed_ip
            )
            .is_err()
        );
        assert!(parse_response(&request, &wire(&wrong_family), |_| false).is_err());

        let aaaa_request = build_query("example.com", RecordType::AAAA).unwrap();
        let mut disallowed_aaaa = response_for(&aaaa_request);
        disallowed_aaaa.add_answer(aaaa("example.com", "fc00::1".parse().unwrap()));
        assert!(
            parse_response(
                &aaaa_request,
                &wire(&disallowed_aaaa),
                crate::oauth::cimd::is_disallowed_ip
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_non_in_relevant_records() {
        let request = request(RecordType::A);
        let mut cname_response = response_for(&request);
        let mut cname = cname("example.com", "alias.example");
        cname.dns_class = DNSClass::CH;
        cname_response.add_answer(cname);
        let mut address_response = response_for(&request);
        let mut address = a("example.com", Ipv4Addr::new(93, 184, 216, 34));
        address.dns_class = DNSClass::CH;
        address_response.add_answer(address);
        assert!(parse_response(&request, &wire(&cname_response), |_| false).is_err());
        assert!(parse_response(&request, &wire(&address_response), |_| false).is_err());
    }

    #[test]
    fn providers_use_the_required_fixed_fallback_order() {
        assert_eq!(
            PROVIDERS.map(|provider| provider.endpoint),
            [
                "https://doh.pub/dns-query",
                "https://dns.alidns.com/dns-query",
                "https://cloudflare-dns.com/dns-query",
            ]
        );
    }
}
