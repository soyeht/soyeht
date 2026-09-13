use soyeht_protocol::{WebElementPackage, WebPickedElement, limits};

use crate::{Error, NormalizedUrl, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct ElementCapture {
    pub element: WebPickedElement,
    pub crop_width: u32,
}

pub fn build_element_package(
    url: &str,
    title: String,
    captures: Vec<ElementCapture>,
) -> Result<WebElementPackage> {
    let url = NormalizedUrl::parse(url)?;
    if captures.is_empty() || captures.len() > limits::WEB_PICKED_ELEMENTS || title.len() > 1024 {
        return Err(Error::ElementLimit);
    }
    let mut elements = Vec::with_capacity(captures.len());
    for capture in captures {
        validate_element(&capture)?;
        elements.push(capture.element);
    }
    let package = WebElementPackage {
        url: url.as_str().to_owned(),
        title,
        elements,
    };
    if serde_json::to_vec(&package)
        .map_err(|_| Error::InvalidElement)?
        .len()
        > limits::JSON_BYTES
    {
        return Err(Error::PayloadTooLarge);
    }
    Ok(package)
}

fn validate_element(capture: &ElementCapture) -> Result<()> {
    let element = &capture.element;
    let image_bytes = element.image.bytes.parse::<usize>().ok();
    let valid = !element.selector.is_empty()
        && element.selector.len() <= 4096
        && element.path.iter().all(|part| part.len() <= 512)
        && element.outer_html.len() <= limits::WEB_OUTER_HTML_BYTES
        && valid_styles(element)
        && valid_accessibility(element)
        && valid_component(element)
        && valid_rect(&element.viewport_rect)
        && valid_rect(&element.page_rect)
        && capture.crop_width <= limits::WEB_PICK_CROP_WIDTH as u32
        && image_bytes.is_some_and(|size| size <= limits::WEB_SCREENSHOT_BYTES)
        && !element.image.attachment_id.is_empty()
        && element.image.attachment_id.len() <= 128
        && element.image.mime_type == "image/png";
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidElement)
    }
}

fn valid_styles(element: &WebPickedElement) -> bool {
    let styles = &element.styles;
    styles.box_sizing.len() <= 256
        && styles.color.len() <= 256
        && styles.background.len() <= 512
        && styles.font.len() <= 512
        && styles.border.len() <= 512
        && styles.border_radius.len() <= 256
        && styles.display.len() <= 256
        && styles.position.len() <= 256
}

fn valid_accessibility(element: &WebPickedElement) -> bool {
    element.accessibility.role.len() <= 256
        && element.accessibility.name.len() <= 2048
        && element.accessibility.state.len() <= 2048
}

fn valid_component(element: &WebPickedElement) -> bool {
    element.component.as_ref().is_none_or(|component| {
        component.framework.len() <= 64
            && component.component.len() <= 512
            && component
                .source_file
                .as_ref()
                .is_none_or(|source| source.len() <= 4096)
            && component.line.is_none_or(|line| line > 0)
    })
}

fn valid_rect(rect: &soyeht_protocol::WebRect) -> bool {
    rect.width <= 16_384 && rect.height <= 16_384
}
