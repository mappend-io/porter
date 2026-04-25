use anyhow::{Result, anyhow};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Semantic {
    Position,
    Normal,
    Tangent,
    TexCoord(u32),
    Color(u32),
    Joints(u32),
    Weights(u32),
    Custom(String),
}

impl fmt::Display for Semantic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Position => f.write_str("POSITION"),
            Self::Normal => f.write_str("NORMAL"),
            Self::Tangent => f.write_str("TANGENT"),
            Self::TexCoord(n) => write!(f, "TEXCOORD_{n}"),
            Self::Color(n) => write!(f, "COLOR_{n}"),
            Self::Joints(n) => write!(f, "JOINTS_{n}"),
            Self::Weights(n) => write!(f, "WEIGHTS_{n}"),
            Self::Custom(s) => f.write_str(s),
        }
    }
}

impl FromStr for Semantic {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "" => Err(anyhow!("Empty semantic")),
            "POSITION" => Ok(Self::Position),
            "NORMAL" => Ok(Self::Normal),
            "TANGENT" => Ok(Self::Tangent),
            _ => {
                if let Some(rest) = s.strip_prefix("TEXCOORD_") {
                    rest.parse()
                        .map(Self::TexCoord)
                        .map_err(|e| anyhow!("Invalid index in semantic {:?}: {}", s.to_owned(), e))
                } else if let Some(rest) = s.strip_prefix("COLOR_") {
                    rest.parse()
                        .map(Self::Color)
                        .map_err(|e| anyhow!("Invalid index in semantic {:?}: {}", s.to_owned(), e))
                } else if let Some(rest) = s.strip_prefix("JOINTS_") {
                    rest.parse()
                        .map(Self::Joints)
                        .map_err(|e| anyhow!("Invalid index in semantic {:?}: {}", s.to_owned(), e))
                } else if let Some(rest) = s.strip_prefix("WEIGHTS_") {
                    rest.parse()
                        .map(Self::Weights)
                        .map_err(|e| anyhow!("Invalid index in semantic {:?}: {}", s.to_owned(), e))
                } else {
                    Ok(Self::Custom(s.to_owned()))
                }
            }
        }
    }
}

impl Serialize for Semantic {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Semantic {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(deserializer)?;
        Self::from_str(s).map_err(serde::de::Error::custom)
    }
}
