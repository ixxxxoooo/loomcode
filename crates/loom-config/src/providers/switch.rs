//! 「当前服务商」:Claude 官方登录也算一个服务商(`None`),模型选择器按服务商分组、选别家的模型即切换服务商。
//!
//! 磁盘格式沿用 `mapping.enabled` + `mapping.provider`(未启用 = Claude 官方),这里只提供语义化的读写与分组。
//!
//! @author ixxxxoooo

use super::{ProviderStore, SLOTS};
use std::collections::HashMap;

/// 模型选择器里的一个服务商分组(不含 Claude 官方,官方的模型由 claude 自己报)。
#[derive(Debug, Clone, PartialEq)]
pub struct PickerGroup {
    /// 服务商 id
    pub id: String,
    /// 显示名
    pub name: String,
    /// 可选模型(当前服务商时按「各槽位 → 清单其余」去重,否则就是清单)
    pub models: Vec<String>,
    /// 不能切过去的原因(缺 Key / 缺地址 / 没有模型);`None` 表示可用
    pub blocked: Option<String>,
}

impl ProviderStore {
    /// 当前使用的服务商 id;`None` 表示 Claude 官方登录。
    pub fn active_id(&self) -> Option<&str> {
        let m = &self.mapping;
        (m.enabled && self.get(&m.provider).is_some()).then_some(m.provider.as_str())
    }

    /// 当前服务商的显示名(Claude 官方为 `None`)。
    pub fn active_name(&self) -> Option<&str> {
        self.active_id().and_then(|id| self.get(id)).map(|p| p.name.as_str())
    }

    /// 切换当前服务商。
    ///
    /// 参数:`provider` 目标服务商(`None` 回到 Claude 官方);`model` 要用的模型(只对第三方服务商有意义,写进「默认」槽位)。
    /// 换到另一家时,其余槽位里不属于新服务商清单的模型一并改成该模型,避免把旧服务商的模型名发给新服务商。
    pub fn activate(&mut self, provider: Option<&str>, model: Option<&str>) {
        let Some(id) = provider.filter(|id| self.get(id).is_some()) else {
            self.mapping.enabled = false;
            return;
        };
        let models = self.get(id).map(|p| p.models.clone()).unwrap_or_default();
        let same = self.mapping.provider == id;
        let pick = model.map(str::to_string).filter(|m| !m.trim().is_empty())
            .or_else(|| Some(self.mapping.default.clone()).filter(|d| same && !d.is_empty()))
            .or_else(|| models.first().cloned())
            .unwrap_or_default();
        let m = &mut self.mapping;
        m.enabled = true;
        m.provider = id.to_string();
        m.default = pick.clone();
        if !same {
            for (slot, ..) in SLOTS {
                let v = m.slot(slot).trim().to_string();
                if !v.is_empty() && !models.contains(&v) {
                    m.set_slot(slot, pick.clone());
                }
            }
        }
    }

    /// 模型选择器的服务商分组(按添加顺序)。
    ///
    /// 参数:`keys` 各服务商的 API Key(只看有没有)。返回:每个服务商一组。
    pub fn picker_groups(&self, keys: &HashMap<String, String>) -> Vec<PickerGroup> {
        let active = self.active_id();
        self.providers.iter().map(|p| {
            let models = if active == Some(p.id.as_str()) {
                self.mapped_models().unwrap_or_default().into_iter().map(|(n, _)| n).collect()
            } else {
                p.models.clone()
            };
            let blocked = if keys.get(&p.id).is_none_or(|k| k.trim().is_empty()) {
                Some("未填写 API Key".to_string())
            } else if p.base_url.trim().is_empty() {
                Some("未填写 Base URL".to_string())
            } else if models.is_empty() {
                Some("模型清单为空".to_string())
            } else {
                None
            };
            PickerGroup { id: p.id.clone(), name: p.name.clone(), models, blocked }
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{AuthHeader, ModelMapping, Provider};
    use super::*;

    fn store() -> ProviderStore {
        let p = |id: &str, name: &str, models: &[&str]| Provider {
            id: id.into(), name: name.into(), base_url: "https://x".into(), auth: AuthHeader::Bearer,
            models: models.iter().map(|s| s.to_string()).collect(),
        };
        ProviderStore {
            providers: vec![p("ds", "DeepSeek", &["deepseek-chat", "deepseek-reasoner"]), p("glm", "GLM", &["glm-4.6"])],
            mapping: ModelMapping { provider: "ds".into(), default: "deepseek-chat".into(), opus: "deepseek-reasoner".into(), ..Default::default() },
        }
    }

    #[test]
    fn 未启用即官方() {
        let mut s = store();
        assert_eq!(s.active_id(), None);
        s.mapping.enabled = true;
        assert_eq!(s.active_name(), Some("DeepSeek"));
        s.mapping.provider = "gone".into();
        assert_eq!(s.active_id(), None, "指向已删除的服务商视为官方");
    }

    #[test]
    fn 切换服务商改写槽位() {
        let mut s = store();
        s.activate(Some("ds"), Some("deepseek-reasoner"));
        assert_eq!((s.active_id(), s.mapping.default.as_str(), s.mapping.opus.as_str()), (Some("ds"), "deepseek-reasoner", "deepseek-reasoner"));
        s.activate(Some("glm"), None);
        assert_eq!((s.mapping.default.as_str(), s.mapping.opus.as_str()), ("glm-4.6", "glm-4.6"), "旧服务商的模型被替换");
        assert!(s.mapping.haiku.is_empty(), "不设置的槽位保持不设置");
        s.activate(None, None);
        assert_eq!(s.active_id(), None);
        assert_eq!(s.mapping.provider, "glm", "回到官方时保留映射");
        s.activate(Some("nope"), None);
        assert_eq!(s.active_id(), None);
    }

    #[test]
    fn 分组标出不可用原因() {
        let s = store();
        let keys = HashMap::from([("ds".to_string(), "k".to_string())]);
        let g = s.picker_groups(&keys);
        assert_eq!(g.len(), 2);
        assert!(g[0].blocked.is_none());
        assert_eq!(g[0].models, vec!["deepseek-chat", "deepseek-reasoner"]);
        assert_eq!(g[1].blocked.as_deref(), Some("未填写 API Key"));
    }
}
