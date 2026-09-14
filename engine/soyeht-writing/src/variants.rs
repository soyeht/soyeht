use crate::{Error, Result};
use soyeht_protocol::{FileAuthor, WritingLineVariants, WritingVariant, limits};

#[derive(Debug, Clone, PartialEq)]
pub struct VariantSet {
    variants: WritingLineVariants,
}

impl VariantSet {
    pub fn new(
        line_id: String,
        line: u32,
        canonical: String,
        author: FileAuthor,
        variant_id: String,
    ) -> Result<Self> {
        validate_text(&canonical)?;
        Ok(Self {
            variants: WritingLineVariants {
                line_id,
                line,
                alternates: vec![WritingVariant {
                    variant_id,
                    text: canonical,
                    author,
                }],
                selected_index: 0,
            },
        })
    }

    pub fn restore(variants: WritingLineVariants) -> Result<Self> {
        validate(&variants)?;
        Ok(Self { variants })
    }

    pub fn add(&mut self, variant: WritingVariant) -> Result<()> {
        validate_text(&variant.text)?;
        if self.variants.alternates.len() >= limits::WRITING_VARIANTS_PER_LINE {
            return Err(Error::VariantLimit);
        }
        if self
            .variants
            .alternates
            .iter()
            .any(|existing| existing.variant_id == variant.variant_id)
        {
            return Err(Error::InvalidVariantSelection);
        }
        self.variants.alternates.push(variant);
        Ok(())
    }

    pub fn select(&mut self, variant_id: &str) -> Result<&str> {
        let index = self
            .variants
            .alternates
            .iter()
            .position(|variant| variant.variant_id == variant_id)
            .ok_or(Error::VariantNotFound)?;
        self.variants.selected_index = u32::try_from(index).map_err(|_| Error::VariantLimit)?;
        Ok(&self.variants.alternates[index].text)
    }

    pub fn remove(&mut self, variant_id: &str) -> Result<&str> {
        if self.variants.alternates.len() <= 1 {
            return Err(Error::CannotRemoveOnlyVariant);
        }
        let index = self
            .variants
            .alternates
            .iter()
            .position(|variant| variant.variant_id == variant_id)
            .ok_or(Error::VariantNotFound)?;
        self.variants.alternates.remove(index);
        let selected = self.variants.selected_index as usize;
        let next = if index < selected {
            selected - 1
        } else if index == selected {
            index.min(self.variants.alternates.len() - 1)
        } else {
            selected
        };
        self.variants.selected_index = u32::try_from(next).map_err(|_| Error::VariantLimit)?;
        Ok(&self.variants.alternates[next].text)
    }

    pub fn selected(&self) -> &str {
        let index = self.variants.selected_index as usize;
        self.variants
            .alternates
            .get(index)
            .map_or("", |variant| variant.text.as_str())
    }

    pub fn public(&self) -> WritingLineVariants {
        self.variants.clone()
    }
}

fn validate(variants: &WritingLineVariants) -> Result<()> {
    if variants.alternates.is_empty()
        || variants.alternates.len() > limits::WRITING_VARIANTS_PER_LINE
        || variants.selected_index as usize >= variants.alternates.len()
    {
        return Err(Error::InvalidVariantSelection);
    }
    for variant in &variants.alternates {
        validate_text(&variant.text)?;
    }
    Ok(())
}

fn validate_text(text: &str) -> Result<()> {
    if text.is_empty() || text.len() > limits::WRITING_VARIANT_BYTES || text.contains('\n') {
        return Err(Error::VariantTooLarge);
    }
    Ok(())
}
