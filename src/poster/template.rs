//! 海报模板，对应 PHP `PosterTemplate`：`{width, height, elements}` 的读写与变量替换。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::{PosterError, Result};

use super::elements::Element;

/// 模板里未写尺寸时的默认值（同 PHP 的硬编码 750×1334）。
pub const DEFAULT_WIDTH: u32 = 750;
/// 见 [`DEFAULT_WIDTH`]。
pub const DEFAULT_HEIGHT: u32 = 1334;

/// 海报模板：尺寸 + 元素定义。
#[derive(Debug, Clone, PartialEq)]
pub struct PosterTemplate {
    width: u32,
    height: u32,
    elements: Vec<Element>,
}

impl PosterTemplate {
    /// 直接由尺寸与元素构建。
    pub fn new(width: u32, height: u32, elements: Vec<Element>) -> Self {
        Self {
            width,
            height,
            elements,
        }
    }

    /// 从 `{"width":…,"height":…,"elements":[…]}` 构建；尺寸缺省 750×1334。
    ///
    /// 元素按 `type` 分派给注册表，未知类型 / 缺少 `type` 都报错（不静默丢弃）。
    pub fn from_config(config: Value) -> Result<Self> {
        let object = config
            .as_object()
            .ok_or_else(|| PosterError::Template("模板必须是 JSON 对象".into()))?;
        let width = object
            .get("width")
            .and_then(Value::as_u64)
            .map(|v| v as u32)
            .unwrap_or(DEFAULT_WIDTH);
        let height = object
            .get("height")
            .and_then(Value::as_u64)
            .map(|v| v as u32)
            .unwrap_or(DEFAULT_HEIGHT);

        let defs = object
            .get("elements")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut elements = Vec::with_capacity(defs.len());
        for (index, def) in defs.into_iter().enumerate() {
            let element = Element::from_def(def)
                .map_err(|e| PosterError::Template(format!("模板元素 #{index}: {e}")))?;
            elements.push(element);
        }
        Ok(Self {
            width,
            height,
            elements,
        })
    }

    /// 从 JSON 文本构建（解析失败报错，不静默回退默认模板）。
    pub fn from_json(json: &str) -> Result<Self> {
        Self::from_config(serde_json::from_str(json)?)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// 元素定义。
    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    /// 展开模板：元素副本（可追加到 `existing` 之后）+ `{{var}}` 替换。
    ///
    /// `existing` 供 `PosterBuilder::replace_elements(false)` 的追加语义使用。
    pub fn build(
        &self,
        variables: &BTreeMap<String, String>,
        existing: Vec<Element>,
    ) -> Result<Vec<Element>> {
        let mut elements = existing;
        for element in &self.elements {
            let mut element = element.clone();
            element.resolve_vars(variables)?;
            elements.push(element);
        }
        Ok(elements)
    }

    /// 导出为 `Value`（元素序列化失败按 `null` 兜底，正常元素不会走到）。
    pub fn to_array(&self) -> Value {
        let elements: Vec<Value> = self
            .elements
            .iter()
            .map(|element| serde_json::to_value(element).unwrap_or(Value::Null))
            .collect();
        serde_json::json!({
            "width": self.width,
            "height": self.height,
            "elements": elements,
        })
    }

    /// 导出为 JSON 文本（缩进格式，同 PHP 的 `JSON_PRETTY_PRINT`）。
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.to_array())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_type_is_an_error_with_index() {
        let err = PosterTemplate::from_config(serde_json::json!({
            "elements": [{"x": 1}]
        }))
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("#0") && message.contains("type"), "{message}");
    }

    #[test]
    fn sizes_default_like_php() {
        let template = PosterTemplate::from_config(serde_json::json!({})).unwrap();
        assert_eq!((template.width(), template.height()), (750, 1334));
    }
}
