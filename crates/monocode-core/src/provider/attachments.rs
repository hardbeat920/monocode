//! Translate the same validated local images into each transport's wire format.
use super::*;
impl Request {
    pub(super) fn codex_input(&self) -> Value {
        let mut blocks = vec![json!({"type":"text","text":self.prompt})];
        blocks.extend(
            self.settings
                .attachments
                .iter()
                .filter(|p| crate::composer::image_mime(p).is_some())
                .map(|p| json!({"type":"localImage","path":p})),
        );
        json!(blocks)
    }
    pub(super) fn claude_content(&self, prompt: &str) -> Result<Value, String> {
        if !self
            .settings
            .attachments
            .iter()
            .any(|p| crate::composer::image_mime(p).is_some())
        {
            return Ok(json!(prompt));
        }
        let mut blocks = vec![json!({"type":"text","text":prompt})];
        for path in &self.settings.attachments {
            if let Some(mime) = crate::composer::image_mime(path) {
                blocks.push(json!({"type":"image","source":{"type":"base64","media_type":mime,"data":crate::composer::image_data(path)?}}));
            }
        }
        Ok(json!(blocks))
    }
    pub(super) fn acp_content(&self) -> Result<Value, String> {
        let mut blocks = vec![json!({"type":"text","text":self.prompt})];
        for path in &self.settings.attachments {
            if let Some(mime) = crate::composer::image_mime(path) {
                blocks.push(json!({"type":"image","mimeType":mime,"data":crate::composer::image_data(path)?}));
            }
        }
        Ok(json!(blocks))
    }
    pub(super) fn pi_images(&self) -> Result<Value, String> {
        let mut blocks = Vec::new();
        for path in &self.settings.attachments {
            if let Some(mime) = crate::composer::image_mime(path) {
                blocks.push(json!({"type":"image","mimeType":mime,"data":crate::composer::image_data(path)?}));
            }
        }
        Ok(json!(blocks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn images_use_native_transport_blocks_and_text_stays_text() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("image.png");
        std::fs::write(&path, [137, 80, 78, 71]).unwrap();
        let mut request = Request {
            kind: Kind::Codex,
            binary: "unused".into(),
            cwd: temp.path().into(),
            prompt: "Look at this".into(),
            provider_id: None,
            model: None,
            settings: Settings::default(),
        };
        assert_eq!(request.claude_content("hello").unwrap(), json!("hello"));
        request.settings.attachments.push(path.clone());
        assert_eq!(
            request.codex_input()[1],
            json!({"type":"localImage","path":path})
        );
        assert_eq!(
            request.claude_content("hello").unwrap()[1]["source"]["data"],
            "iVBORw=="
        );
        assert_eq!(request.acp_content().unwrap()[1]["mimeType"], "image/png");
        assert_eq!(request.pi_images().unwrap()[0]["type"], "image");
    }
}
