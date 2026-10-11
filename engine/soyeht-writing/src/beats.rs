use crate::{Error, Result};
use soyeht_protocol::{BeatCard, limits};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct BeatBoard {
    cards: Vec<BeatCard>,
}

impl BeatBoard {
    pub fn restore(mut cards: Vec<BeatCard>, scene_ids: &[String]) -> Result<Self> {
        if cards.len() > limits::WRITING_BEAT_CARDS {
            return Err(Error::BeatLimit);
        }
        let scenes: HashSet<&str> = scene_ids.iter().map(String::as_str).collect();
        let mut ids = HashSet::new();
        let mut links = 0usize;
        for card in &cards {
            validate_card(card, &scenes)?;
            if !ids.insert(card.beat_id.as_str()) {
                return Err(Error::InvalidBeat);
            }
            links += usize::from(card.scene_id.is_some());
        }
        if links > limits::WRITING_SCENE_LINKS {
            return Err(Error::LinkLimit);
        }
        cards.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then(left.beat_id.cmp(&right.beat_id))
        });
        normalize_order(&mut cards)?;
        Ok(Self { cards })
    }

    pub fn put(&mut self, card: BeatCard, scene_ids: &[String]) -> Result<()> {
        let scenes: HashSet<&str> = scene_ids.iter().map(String::as_str).collect();
        validate_card(&card, &scenes)?;
        let existing = self
            .cards
            .iter()
            .position(|item| item.beat_id == card.beat_id);
        if existing.is_none() && self.cards.len() >= limits::WRITING_BEAT_CARDS {
            return Err(Error::BeatLimit);
        }
        if let Some(index) = existing {
            self.cards[index] = card;
        } else {
            self.cards.push(card);
        }
        self.cards.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then(left.beat_id.cmp(&right.beat_id))
        });
        normalize_order(&mut self.cards)
    }

    pub fn delete(&mut self, beat_id: &str) -> Result<BeatCard> {
        let index = self
            .cards
            .iter()
            .position(|card| card.beat_id == beat_id)
            .ok_or(Error::BeatNotFound)?;
        let removed = self.cards.remove(index);
        normalize_order(&mut self.cards)?;
        Ok(removed)
    }

    pub fn move_to(&mut self, beat_id: &str, index: usize) -> Result<()> {
        let current = self
            .cards
            .iter()
            .position(|card| card.beat_id == beat_id)
            .ok_or(Error::BeatNotFound)?;
        let card = self.cards.remove(current);
        let target = index.min(self.cards.len());
        self.cards.insert(target, card);
        normalize_order(&mut self.cards)
    }

    pub fn cards(&self) -> &[BeatCard] {
        &self.cards
    }
}

fn validate_card(card: &BeatCard, scenes: &HashSet<&str>) -> Result<()> {
    if card.beat_id.is_empty()
        || card.title.is_empty()
        || card.title.len() > 512
        || card.body.len() > limits::WRITING_VARIANT_BYTES
    {
        return Err(Error::InvalidBeat);
    }
    if card
        .scene_id
        .as_deref()
        .is_some_and(|scene| !scenes.contains(scene))
    {
        return Err(Error::SceneNotFound);
    }
    Ok(())
}

fn normalize_order(cards: &mut [BeatCard]) -> Result<()> {
    for (index, card) in cards.iter_mut().enumerate() {
        card.order = u32::try_from(index).map_err(|_| Error::BeatLimit)?;
    }
    Ok(())
}
