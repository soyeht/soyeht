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
    if captures.is_empty()
        || captures.len() > limits::WEB_PICKED_ELEMENTS
        || title.len() > limits::WEB_TITLE_BYTES
    {
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
        && element.selector.len() <= limits::WEB_FIELD_BYTES
        && element
            .path
            .iter()
            .all(|part| part.len() <= limits::WEB_PART_BYTES)
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
    styles.box_sizing.len() <= limits::WEB_TOKEN_BYTES
        && styles.color.len() <= limits::WEB_TOKEN_BYTES
        && styles.background.len() <= limits::WEB_PART_BYTES
        && styles.font.len() <= limits::WEB_PART_BYTES
        && styles.border.len() <= limits::WEB_PART_BYTES
        && styles.border_radius.len() <= limits::WEB_TOKEN_BYTES
        && styles.display.len() <= limits::WEB_TOKEN_BYTES
        && styles.position.len() <= limits::WEB_TOKEN_BYTES
}

fn valid_accessibility(element: &WebPickedElement) -> bool {
    element.accessibility.role.len() <= limits::WEB_TOKEN_BYTES
        && element.accessibility.name.len() <= limits::WEB_LABEL_BYTES
        && element.accessibility.state.len() <= limits::WEB_LABEL_BYTES
}

fn valid_component(element: &WebPickedElement) -> bool {
    element.component.as_ref().is_none_or(|component| {
        component.framework.len() <= 64
            && component.component.len() <= limits::WEB_PART_BYTES
            && component
                .source_file
                .as_ref()
                .is_none_or(|source| source.len() <= limits::WEB_FIELD_BYTES)
            && component.line.is_none_or(|line| line > 0)
    })
}

fn valid_rect(rect: &soyeht_protocol::WebRect) -> bool {
    rect.width as usize <= limits::WEB_EXTENT_MAX && rect.height as usize <= limits::WEB_EXTENT_MAX
}
