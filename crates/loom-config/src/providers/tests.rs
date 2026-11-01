//! 服务商配置、映射环境变量与密钥文件权限的测试。
//!
//! @author ixxxxoooo

use super::*;
use std::collections::HashMap;

fn store() -> ProviderStore {
    ProviderStore {
        providers: vec![Provider { id: "ds".into(), name: "DeepSeek".into(), base_url: "https://api.deepseek.com/anthropic/".into(), auth: AuthHeader::Bearer, models: vec!["deepseek-chat".into()] }],
        mapping: ModelMapping { enabled: true, provider: "ds".into(), default: "deepseek-chat".into(), haiku: "deepseek-chat".into(), ..Default::default() },
    }
}

#[test]
fn 映射生效时注入环境变量() {
    let keys = HashMap::from([("ds".to_string(), "sk-test".to_string())]);
    let env = store().mapping_env(&keys);
    assert!(env.contains(&("ANTHROPIC_BASE_URL".into(), "https://api.deepseek.com/anthropic".into())));
    assert!(env.contains(&("ANTHROPIC_AUTH_TOKEN".into(), "sk-test".into())));
    assert!(env.contains(&("ANTHROPIC_MODEL".into(), "deepseek-chat".into())));
    assert!(env.contains(&("ANTHROPIC_DEFAULT_HAIKU_MODEL".into(), "deepseek-chat".into())));
    assert!(!env.iter().any(|(k, _)| k == "ANTHROPIC_DEFAULT_OPUS_MODEL"));
}

#[test]
fn 映射未生效的原因() {
    let mut s = store();
    assert!(s.mapping_status(&HashMap::new()).unwrap_err().contains("API Key"));
    assert!(s.mapping_env(&HashMap::new()).is_empty());
    let keys = HashMap::from([("ds".to_string(), "k".to_string())]);
    s.mapping.default.clear();
    assert!(s.mapping_status(&keys).unwrap_err().contains("默认"));
    s.mapping.enabled = false;
    assert!(s.mapping_status(&keys).unwrap_err().contains("官方"));
}

#[test]
fn 官方接口用_x_api_key() {
    let mut s = store();
    s.providers[0].auth = AuthHeader::XApiKey;
    let env = s.mapping_env(&HashMap::from([("ds".to_string(), "k".to_string())]));
    assert!(env.iter().any(|(k, _)| k == "ANTHROPIC_API_KEY"));
    assert!(!env.iter().any(|(k, _)| k == "ANTHROPIC_AUTH_TOKEN"));
}

#[test]
fn 新_id_不重复() {
    let s = store();
    assert_eq!(s.new_id("智谱 GLM"), "glm");
    assert_eq!(s.new_id("阿里"), "provider");
    let mut s2 = s.clone();
    s2.providers.push(Provider { id: "glm".into(), ..Default::default() });
    assert_eq!(s2.new_id("GLM"), "glm-2");
}

#[test]
fn 保存读回且密钥文件为_0600() {
    let dir = std::env::temp_dir().join(format!("loom-providers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (cfg, keys_path) = (dir.join("providers.json"), dir.join("provider-keys.json"));
    store().save(&cfg).unwrap();
    assert_eq!(ProviderStore::load(&cfg), store());
    let keys = HashMap::from([("ds".to_string(), "sk-secret".to_string())]);
    save_keys(&keys_path, &keys).unwrap();
    assert_eq!(load_keys(&keys_path), keys);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&keys_path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert!(!std::fs::read_to_string(&cfg).unwrap().contains("sk-secret"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn 映射生效时关闭工具搜索() {
    let env = store().mapping_env(&HashMap::from([("ds".to_string(), "k".to_string())]));
    assert!(env.contains(&("ENABLE_TOOL_SEARCH".to_string(), "false".to_string())));
}

#[test]
fn 模型选择器按映射去重并解析别名() {
    let mut s = store();
    s.mapping.enabled = true;
    s.mapping.opus = "deepseek-reasoner".into();
    s.mapping.sonnet = "deepseek-chat".into();
    s.mapping.haiku = "deepseek-chat".into();
    let names: Vec<String> = s.mapped_models().unwrap().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names.iter().filter(|n| *n == "deepseek-chat").count(), 1);
    assert!(names.contains(&"deepseek-reasoner".to_string()));
    assert_eq!(s.resolve_model("opus"), "deepseek-reasoner");
    assert_eq!(s.resolve_model("sonnet[1m]"), "deepseek-chat");
    assert_eq!(s.resolve_model("custom-x"), "custom-x");
    s.mapping.enabled = false;
    assert!(s.mapped_models().is_none());
}
