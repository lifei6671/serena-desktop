use super::*;

pub(crate) fn assert_image(result: &CallToolResult, mime: &str) -> DynamicImage {
    assert_ne!(result.is_error, Some(true));
    assert!(result.structured_content.is_none());
    assert_eq!(result.content.len(), 1);
    let ContentBlock::Image(content) = &result.content[0] else {
        panic!("expected native ImageContent")
    };
    assert_eq!(content.mime_type, mime);
    let bytes = STANDARD.decode(&content.data).unwrap();
    assert!(bytes.len() <= MAX_OUTPUT);
    assert!(serde_json::to_vec(result).unwrap().len() < 9 * 1024 * 1024);
    image::load_from_memory(&bytes).unwrap()
}

pub(crate) fn poc_image() -> DynamicImage {
    let mut image = image::RgbImage::from_pixel(640, 320, image::Rgb([255, 255, 255]));
    // A tiny test-only bitmap alphabet keeps the POC independent of installed fonts.
    let glyphs = [
        ('S', [31, 16, 16, 31, 1, 1, 31]),
        ('E', [31, 16, 16, 30, 16, 16, 31]),
        ('R', [30, 17, 17, 30, 20, 18, 17]),
        ('N', [17, 25, 25, 21, 19, 19, 17]),
        ('A', [14, 17, 17, 31, 17, 17, 17]),
        ('I', [31, 4, 4, 4, 4, 4, 31]),
        ('M', [17, 27, 21, 21, 17, 17, 17]),
        ('G', [14, 17, 16, 23, 17, 17, 14]),
        ('T', [31, 4, 4, 4, 4, 4, 4]),
        ('9', [14, 17, 17, 15, 1, 1, 14]),
        ('2', [14, 17, 1, 2, 4, 8, 31]),
        ('7', [31, 1, 2, 4, 8, 8, 8]),
        ('4', [2, 6, 10, 18, 31, 2, 2]),
    ];
    for (text, top) in [("SERENA IMAGE TEST", 24), ("9274", 100)] {
        for (index, ch) in text.chars().enumerate() {
            if let Some((_, rows)) = glyphs.iter().find(|(c, _)| *c == ch) {
                for (y, row) in rows.iter().enumerate() {
                    for x in 0..5 {
                        if row & (1 << (4 - x)) != 0 {
                            for dy in 0..5 {
                                for dx in 0..5 {
                                    image.put_pixel(
                                        24 + index as u32 * 35 + x * 5 + dx,
                                        top + y as u32 * 5 + dy,
                                        image::Rgb([0, 0, 0]),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for y in 160_i32..300 {
        for x in 250_i32..390 {
            if (x - 320).pow(2) + (y - 230).pow(2) <= 65_i32.pow(2) {
                image.put_pixel(x as u32, y as u32, image::Rgb([255, 0, 0]));
            }
        }
    }
    DynamicImage::ImageRgb8(image)
}

#[test]
fn supported_formats_are_native_decodable_content_and_ignore_extension() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for (format, mime) in [
        (ImageFormat::Png, "image/png"),
        (ImageFormat::Jpeg, "image/jpeg"),
        (ImageFormat::WebP, "image/png"),
    ] {
        poc_image()
            .save_with_format(root.join("wrong.txt"), format)
            .unwrap();
        let decoded = assert_image(
            &read(&root, "wrong.txt", &CancellationToken::new()).unwrap(),
            mime,
        );
        assert_eq!((decoded.width(), decoded.height()), (640, 320));
    }
}

#[test]
fn invalid_paths_files_and_media_fail() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("fake.png"), b"text").unwrap();
    std::fs::write(root.join("broken.png"), b"\x89PNG\r\n\x1a\ncorrupt").unwrap();
    std::fs::write(root.join("unsupported.gif"), b"GIF89a").unwrap();
    for (path, code) in [
        ("../secret.png", "INVALID_PATH"),
        ("../../secret.png", "INVALID_PATH"),
        ("C:\\secret.png", "INVALID_PATH"),
        ("missing.png", "INVALID_PATH"),
        ("", "INVALID_PATH"),
        ("fake.png", "UNSUPPORTED_MEDIA_TYPE"),
        ("broken.png", "IMAGE_DECODE_FAILED"),
        ("unsupported.gif", "UNSUPPORTED_MEDIA_TYPE"),
    ] {
        assert!(
            read(&root, path, &CancellationToken::new())
                .unwrap_err()
                .starts_with(code),
            "{path}"
        );
    }
    assert!(
        read(
            &root,
            root.join("fake.png").to_str().unwrap(),
            &CancellationToken::new()
        )
        .unwrap_err()
        .starts_with("INVALID_PATH")
    );
    #[cfg(windows)]
    assert!(
        read(&root, r"\\server\share\a.png", &CancellationToken::new())
            .unwrap_err()
            .starts_with("INVALID_PATH")
    );
}

#[test]
fn input_and_decoded_pixel_limits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::File::create(root.join("huge.png"))
        .unwrap()
        .set_len(MAX_INPUT + 1)
        .unwrap();
    assert!(
        read(&root, "huge.png", &CancellationToken::new())
            .unwrap_err()
            .starts_with("OUTPUT_LIMIT_EXCEEDED")
    );
    DynamicImage::new_luma8(8000, 5001)
        .save(root.join("pixels.png"))
        .unwrap();
    assert!(
        read(&root, "pixels.png", &CancellationToken::new())
            .unwrap_err()
            .starts_with("OUTPUT_LIMIT_EXCEEDED")
    );
}

#[test]
fn large_image_is_resized_and_output_failure_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    DynamicImage::new_rgb8(3000, 1500)
        .save(root.join("large.png"))
        .unwrap();
    let result = read(&root, "large.png", &CancellationToken::new()).unwrap();
    let image = assert_image(&result, "image/png");
    assert_eq!((image.width(), image.height()), (2560, 1280));
    for format in [ImageFormat::Png, ImageFormat::Jpeg] {
        assert_eq!(
            encode(poc_image(), format, 1, &CancellationToken::new()).unwrap_err(),
            "OUTPUT_LIMIT_EXCEEDED"
        );
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(read(&root, "large.png", &cancel).unwrap_err(), "CANCELLED");
}

#[test]
fn escaping_directory_link_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("workspace");
    let outside = dir.path().join("outside");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&outside).unwrap();
    poc_image().save(outside.join("secret.png")).unwrap();
    let link = root.join("link");
    #[cfg(windows)]
    {
        // Junctions do not require Developer Mode or symlink privileges.
        let status = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .status()
            .unwrap();
        assert!(status.success());
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let root = root.canonicalize().unwrap();
    for path in ["link/secret.png", "link/../outside.png"] {
        assert!(
            read(&root, path, &CancellationToken::new())
                .unwrap_err()
                .starts_with("INVALID_PATH")
        );
    }
    #[cfg(windows)]
    std::fs::remove_dir(link).unwrap();
}

#[test]
fn oversized_encoding_shrinks_to_fit_real_budget() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut state = 9274_u32;
    let noise = image::RgbImage::from_fn(1600, 1600, |_, _| {
        let mut rgb = [0; 3];
        for channel in &mut rgb {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *channel = state as u8;
        }
        image::Rgb(rgb)
    });
    noise.save(root.join("noise.png")).unwrap();
    assert!(std::fs::metadata(root.join("noise.png")).unwrap().len() > MAX_OUTPUT as u64);
    let result = read(&root, "noise.png", &CancellationToken::new()).unwrap();
    let decoded = assert_image(&result, "image/png");
    assert!(decoded.width() < 1600);
    assert_eq!(decoded.width(), decoded.height());
}

#[test]
fn reencoding_removes_original_jpeg_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("metadata.jpg");
    poc_image().save(&path).unwrap();
    let original = std::fs::read(&path).unwrap();
    let metadata = b"Exif\0\0SERENA_PRIVATE_GPS_DEVICE_METADATA";
    let mut bytes = original[..2].to_vec();
    bytes.extend_from_slice(&[0xff, 0xe1]);
    bytes.extend_from_slice(&((metadata.len() + 2) as u16).to_be_bytes());
    bytes.extend_from_slice(metadata);
    bytes.extend_from_slice(&original[2..]);
    std::fs::write(&path, bytes).unwrap();
    let result = read(&root, "metadata.jpg", &CancellationToken::new()).unwrap();
    assert_image(&result, "image/jpeg");
    let ContentBlock::Image(content) = &result.content[0] else {
        unreachable!()
    };
    let bytes = STANDARD.decode(&content.data).unwrap();
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Jpeg)
        .into_decoder()
        .unwrap();
    assert!(decoder.exif_metadata().unwrap().is_none());
}
