use std::collections::HashMap;

use crate::GolobulError;

type Color = [f32; 4];

#[derive(Debug, Clone, PartialEq)]
pub enum Variant {
    Image(DiscreteCfg<Image>),
    Bool(DiscreteCfg<bool>),
    TaggedInt(TaggedInt),
    Color(DiscreteCfg<Color>),
    Int(Cfg<i32>),
    Float(Cfg<f32>),
    Vector2(Cfg<[f32; 2]>),
}

impl Variant {
    pub fn adopt(&mut self, other: &Self) -> Result<(), GolobulError> {
        match (self, other) {
            (Variant::Image(_), Variant::Image(_)) => {}
            (Variant::TaggedInt(self_i), Variant::TaggedInt(i)) => {
                if let Some(k) = i.tags.iter().find(|(_, v)| **v == i.value) {
                    if self_i.tags.contains_key(k.0) {
                        self_i.value = i.value;
                    }
                }
            }
            (Variant::Color(self_c), Variant::Color(other)) => {
                self_c.current = other.current;
            }
            (Variant::Int(i_me), Variant::Int(i_other)) => {
                if i_other.current < i_me.max && i_other.current > i_me.min {
                    i_me.current = i_other.current;
                }
            }
            (Variant::Float(i_me), Variant::Float(i_other)) => {
                if i_other.current < i_me.max && i_other.current > i_me.min {
                    i_me.current = i_other.current;
                }
            }
            (Variant::Vector2(i_me), Variant::Vector2(i_other)) => {
                if i_other.current < i_me.max && i_other.current > i_me.min {
                    i_me.current = i_other.current;
                }
            }
            _ => {
                return Err(GolobulError::TypeMismatch);
            }
        }

        Ok(())
    }
}

// Continuous bounded values
#[derive(Debug, Clone, PartialEq)]
pub struct Cfg<T: Clone + PartialEq> {
    pub default: T,
    pub current: T,
    pub min: T,
    pub max: T,
}

// Discrete values
#[derive(Debug, Clone, PartialEq)]
pub struct DiscreteCfg<T: Clone + PartialEq> {
    pub current: T,
    pub default: T,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaggedInt {
    pub value: i32,
    pub default: i32,
    pub tags: HashMap<String, i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Image {
    Input,
    Output,
}

impl<T: Clone + PartialEq> Cfg<T> {
    pub fn new(default: T, min: T, max: T) -> Self {
        Self {
            current: default.clone(),
            default,
            min,
            max,
        }
    }
}

impl<T: Clone + PartialEq> DiscreteCfg<T> {
    pub fn new(current: T) -> Self {
        Self {
            default: current.clone(),
            current,
        }
    }
}

impl TaggedInt {
    pub fn new(default: i32, tags: HashMap<String, i32>) -> Self {
        Self {
            value: default,
            default,
            tags,
        }
    }
}
