declare const __SERENA_DESKTOP_VERSION__: string;

/** 读取 Vite 在编译时注入的版本；测试环境未注入时明确标示为开发版本。 */
export const appVersion =
  typeof __SERENA_DESKTOP_VERSION__ === "string" ? __SERENA_DESKTOP_VERSION__ : "开发版本";
