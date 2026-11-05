//! The image a user turn carries (story:image-input): inline bytes of one of four media types, or
//! an `https` URL the far side fetches.
//!
//! **No image byte and no URL reaches a diagnostic or a `Debug` form.** A URL may carry a signed
//! query, and inline bytes are the caller's content. So `Debug` shows the media type and the
//! length only, every refusal here is fixed text, and the serialized form is read by a reader
//! that refuses a malformed value without quoting it.
use crate::{Error, MAX_IMAGE_BYTES, MAX_IMAGE_URL_BYTES};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The media type of inline image bytes, written as its MIME name.
///
/// The set may grow, so a match on it outside this crate needs a wildcard arm, and that arm
/// refuses the type it does not know, by [`ImageMediaType::name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageMediaType {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageMediaType {
    /// Every media type, in this order.
    pub const ALL: &[Self] = &[Self::Png, Self::Jpeg, Self::Gif, Self::Webp];

    /// The MIME name: `image/png`, `image/jpeg`, `image/gif` or `image/webp`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    /// The media type named exactly so, in lower case; any other name is none of them.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|media_type| media_type.name() == name)
    }
}

const MEDIA_TYPE_REFUSAL: &str =
    "an image media type is none of image/png, image/jpeg, image/gif and image/webp";
const BASE64_REFUSAL: &str = "inline image data is not standard base64";
const WIRE_URL_REFUSAL: &str = "an image URL is neither an https URL nor a base64 data URL of image/png, image/jpeg, image/gif or image/webp";
const URL_REFUSAL: &str = "an image URL must be an https URL of visible ASCII naming a host";
const SOURCE_REFUSAL: &str =
    "an image source is an object of kind bytes (media_type and data) or url (url), each key once";

impl Serialize for ImageMediaType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for ImageMediaType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = deserializer.deserialize_any(Text(MEDIA_TYPE_REFUSAL))?;
        Self::from_name(&name).ok_or_else(|| de::Error::custom(MEDIA_TYPE_REFUSAL))
    }
}

/// Where the image is: its bytes, inline, or an `https` URL the far side fetches.
///
/// A URL carries no media type: no wire has a place for one, and the far side reads the type of
/// what it fetches. Bytes are written as standard base64 in a document.
///
/// The set may grow, so a match on it outside this crate needs a wildcard arm, and that arm
/// refuses the source it does not know rather than sending it.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageSource {
    /// At most [`MAX_IMAGE_BYTES`], and at least one.
    Bytes {
        media_type: ImageMediaType,
        data: Vec<u8>,
    },
    /// `https://`, a host, visible ASCII only, at most [`MAX_IMAGE_URL_BYTES`].
    Url { url: String },
}

impl ImageSource {
    pub fn bytes(media_type: ImageMediaType, data: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            media_type,
            data: data.into(),
        }
    }

    pub fn url(url: impl Into<String>) -> Self {
        Self::Url { url: url.into() }
    }

    /// Inline bytes read from standard base64 text with its padding, as every wire writes them.
    ///
    /// # Errors
    /// Refuses text that would decode past [`MAX_IMAGE_BYTES`], before decoding it, and text that
    /// is not canonical standard base64. Neither refusal quotes the text.
    pub fn from_base64(media_type: ImageMediaType, text: &str) -> Result<Self, Error> {
        if text.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
            return Err(Error::too_large("an inline image exceeds its bound"));
        }
        let data = STANDARD
            .decode(text)
            .map_err(|_| Error::invalid(BASE64_REFUSAL))?;
        Ok(Self::Bytes { media_type, data })
    }

    /// The inline bytes as standard base64 text with its padding; none for a URL.
    pub fn base64(&self) -> Option<String> {
        match self {
            Self::Bytes { data, .. } => Some(STANDARD.encode(data)),
            Self::Url { .. } => None,
        }
    }

    /// The image as the one URL Responses and Chat Completions carry: inline bytes as a base64
    /// `data:` URL of their media type, a URL as itself.
    pub fn to_wire_url(&self) -> String {
        match self {
            Self::Bytes { media_type, data } => {
                format!(
                    "data:{};base64,{}",
                    media_type.name(),
                    STANDARD.encode(data)
                )
            }
            Self::Url { url } => url.clone(),
        }
    }

    /// Reads the one URL Responses and Chat Completions carry: a base64 `data:` URL of one of
    /// the four media types, or an `https` URL. What [`ImageSource::validate`] refuses is left to
    /// it, so a reader that validates the turn refuses it by the same text.
    ///
    /// # Errors
    /// Refuses every other URL, a data URL of another media type or encoding and base64 that is
    /// not canonical, each in fixed text.
    pub fn from_wire_url(text: &str) -> Result<Self, Error> {
        if let Some(data) = text.strip_prefix("data:") {
            let (media_type, encoded) = data
                .split_once(";base64,")
                .and_then(|(name, encoded)| Some((ImageMediaType::from_name(name)?, encoded)))
                .ok_or_else(|| Error::invalid(WIRE_URL_REFUSAL))?;
            return Self::from_base64(media_type, encoded);
        }
        if text.starts_with("https://") {
            return Ok(Self::url(text));
        }
        Err(Error::invalid(WIRE_URL_REFUSAL))
    }

    /// # Errors
    /// Refuses inline bytes that are empty or past [`MAX_IMAGE_BYTES`], and a URL past
    /// [`MAX_IMAGE_URL_BYTES`] or other than `https://`, a host and visible ASCII. Fixed text.
    pub fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Bytes { data, .. } => {
                if data.is_empty() {
                    return Err(Error::invalid("an inline image carries no bytes"));
                }
                if data.len() > MAX_IMAGE_BYTES {
                    return Err(Error::too_large("an inline image exceeds its bound"));
                }
            }
            Self::Url { url } => {
                if url.len() > MAX_IMAGE_URL_BYTES {
                    return Err(Error::too_large("an image URL exceeds its bound"));
                }
                let host = url
                    .strip_prefix("https://")
                    .and_then(|rest| rest.split(['/', '?', '#']).next())
                    .unwrap_or_default();
                if host.is_empty() || !url.bytes().all(|byte| byte.is_ascii_graphic()) {
                    return Err(Error::invalid(URL_REFUSAL));
                }
            }
        }
        Ok(())
    }
}

/// The media type and the length, never a byte or the URL.
impl fmt::Debug for ImageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes { media_type, data } => f
                .debug_struct("Bytes")
                .field("media_type", media_type)
                .field("len", &data.len())
                .finish(),
            Self::Url { url } => f
                .debug_struct("Url")
                .field("len", &url.len())
                .finish_non_exhaustive(),
        }
    }
}

impl Serialize for ImageSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Bytes { media_type, data } => {
                let mut source = serializer.serialize_struct("ImageSource", 3)?;
                source.serialize_field("kind", "bytes")?;
                source.serialize_field("media_type", media_type)?;
                source.serialize_field("data", &STANDARD.encode(data))?;
                source.end()
            }
            Self::Url { url } => {
                let mut source = serializer.serialize_struct("ImageSource", 2)?;
                source.serialize_field("kind", "url")?;
                source.serialize_field("url", url)?;
                source.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for ImageSource {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SourceVisitor)
    }
}

/// A string read without ever quoting it: any other shape, and the string itself where a reader
/// refuses it, is refused by the fixed text it carries.
struct Text(&'static str);

impl<'de> Deserialize<'de> for Redacted {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Text(SOURCE_REFUSAL)).map(Self)
    }
}

/// A string value of an image source, read by [`Text`].
struct Redacted(String);

impl Visitor<'_> for Text {
    type Value = String;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        Ok(value.to_owned())
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<String, E> {
        Ok(value)
    }
    fn visit_bytes<E: de::Error>(self, _: &[u8]) -> Result<String, E> {
        Err(E::custom(self.0))
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<String, E> {
        Err(E::custom(self.0))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<String, E> {
        Err(E::custom(self.0))
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<String, E> {
        Err(E::custom(self.0))
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<String, E> {
        Err(E::custom(self.0))
    }
}

struct SourceVisitor;

impl<'de> Visitor<'de> for SourceVisitor {
    type Value = ImageSource;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(SOURCE_REFUSAL)
    }
    // A string where the object belongs is most likely the URL itself; it is not quoted.
    fn visit_str<E: de::Error>(self, _: &str) -> Result<ImageSource, E> {
        Err(E::custom(SOURCE_REFUSAL))
    }
    fn visit_bytes<E: de::Error>(self, _: &[u8]) -> Result<ImageSource, E> {
        Err(E::custom(SOURCE_REFUSAL))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ImageSource, A::Error> {
        let mut kind = None;
        let mut media_type = None;
        let mut data = None;
        let mut url = None;
        while let Some(Redacted(key)) = map.next_key()? {
            let repeated = match key.as_str() {
                "kind" => kind.replace(map.next_value::<Redacted>()?.0).is_some(),
                "media_type" => media_type
                    .replace(map.next_value::<ImageMediaType>()?)
                    .is_some(),
                "data" => data.replace(map.next_value::<Redacted>()?.0).is_some(),
                "url" => url.replace(map.next_value::<Redacted>()?.0).is_some(),
                _ => true,
            };
            if repeated {
                return Err(de::Error::custom(SOURCE_REFUSAL));
            }
        }
        match (kind.as_deref(), media_type, data, url) {
            (Some("bytes"), Some(media_type), Some(data), None) => {
                ImageSource::from_base64(media_type, &data)
                    .map_err(|error| de::Error::custom(error.message))
            }
            (Some("url"), None, None, Some(url)) => Ok(ImageSource::Url { url }),
            _ => Err(de::Error::custom(SOURCE_REFUSAL)),
        }
    }
}
