pub use semantic::*;
pub mod semantic;

pub use defaults::*;
pub mod defaults;

// Helper to deal with open-ended int enums: we want an enum, but also an Other
// type so we can pattern match
macro_rules! open_int_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident: $repr:ty {
            $( $variant:ident = $value:literal ),* $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        $vis enum $name {
            $( $variant, )*
            Other($repr),
        }

        impl $name {
            pub fn as_int(self) -> $repr {
                match self {
                    $( Self::$variant => $value, )*
                    Self::Other(n) => n,
                }
            }

            pub fn from_int(n: $repr) -> Self {
                match n {
                    $( $value => Self::$variant, )*
                    other => Self::Other(other),
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                self.as_int().serialize(s)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Ok(Self::from_int(<$repr>::deserialize(d)?))
            }
        }
    };
}

// And another for string enums
macro_rules! open_str_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $( $variant:ident = $value:literal ),* $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        $vis enum $name {
            $( $variant, )*
            Other(String),
        }

        impl $name {
            pub fn as_str(&self) -> &str {
                match self {
                    $( Self::$variant => $value, )*
                    Self::Other(s) => s,
                }
            }

            pub fn from_str_open(s: &str) -> Self {
                match s {
                    $( $value => Self::$variant, )*
                    other => Self::Other(other.to_owned()),
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <&str>::deserialize(d)?;
                Ok(Self::from_str_open(s))
            }
        }
    };
}

open_int_enum! {
    pub enum WrapMode: u32 {
        ClampToEdge = 33071,
        MirroredRepeat = 33648,
        Repeat = 10497,
    }
}

open_int_enum! {
    pub enum MagFilter: u32 {
        Nearest = 9728,
        Linear = 9729,
    }
}

open_int_enum! {
    pub enum MinFilter: u32 {
        Nearest = 9728,
        Linear = 9729,
        NearestMipmapNearest = 9984,
        LinearMipmapNearest = 9985,
        NearestMipmapLinear = 9986,
        LinearMipmapLinear = 9987,
    }
}

open_int_enum! {
    pub enum ComponentType: u32 {
        Byte = 5120,
        UnsignedByte = 5121,
        Short = 5122,
        UnsignedShort = 5123,
        UnsignedInt = 5125,
        Float = 5126,
    }
}

impl ComponentType {
    pub fn byte_size(&self) -> Option<u8> {
        Some(match self {
            Self::Byte | Self::UnsignedByte => 1,
            Self::Short | Self::UnsignedShort => 2,
            Self::UnsignedInt => 4,
            Self::Float => 4,
            Self::Other(_) => return None,
        })
    }
}

open_int_enum! {
    pub enum Mode: u32 {
        Points = 0,
        Lines = 1,
        LineLoop = 2,
        LineStrip = 3,
        Triangles = 4,
        TriangleStrip = 5,
        TriangleFan = 6,
    }
}

open_int_enum! {
    pub enum Target: u32 {
        ArrayBuffer = 34962,
        ElementArrayBuffer = 34963,
    }
}

open_str_enum! {
    pub enum Type {
        Scalar = "SCALAR",
        Vec2 = "VEC2",
        Vec3 = "VEC3",
        Vec4 = "VEC4",
        Mat2 = "MAT2",
        Mat3 = "MAT3",
        Mat4 = "MAT4",
    }
}

impl Type {
    pub fn component_count(&self) -> Option<u8> {
        Some(match self {
            Self::Scalar => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 => 4,
            Self::Mat2 => 4,
            Self::Mat3 => 9,
            Self::Mat4 => 16,
            Self::Other(_) => return None,
        })
    }
}

open_str_enum! {
    pub enum MimeType {
        Jpeg = "image/jpeg",
        Png = "image/png",
    }
}

open_str_enum! {
    pub enum AlphaMode {
        Opaque = "OPAQUE",
        Mask = "MASK",
        Blend = "BLEND",
    }
}

#[allow(clippy::derivable_impls)]
impl Default for AlphaMode {
    fn default() -> Self {
        AlphaMode::Opaque
    }
}
