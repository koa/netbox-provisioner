use core::str;
use serde::de;
use serde::de::{Error, Visitor};
use std::{fmt, fmt::Formatter, marker::PhantomData};

macro_rules! create_id {
    ($id:ident) => {
        #[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Default)]
        pub struct $id(pub u32);
        impl From<u32> for $id {
            fn from(value: u32) -> Self {
                $id(value)
            }
        }

        impl<'de> Deserialize<'de> for $id {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer
                    .deserialize_str(id::IdVisitor(std::marker::PhantomData::<$id>::default()))
            }
        }
        impl_scalar!($id, schema::ID);
    };
}

macro_rules! create_from_str {
    ($wrapper: ident, $base: ident, $message: literal) => {
        #[derive(Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
        pub struct $wrapper(pub $base);
        impl FromStr for $wrapper {
            type Err = <$base as FromStr>::Err;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                $base::from_str(s).map($wrapper)
            }
        }
        impl Display for $wrapper {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.0, f)
            }
        }
        impl Debug for $wrapper {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(&self.0, f)
            }
        }

        impl Deref for $wrapper {
            type Target = $base;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
        impl<'de> Deserialize<'de> for $wrapper {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_str(id::FromStrVisitor::new("Hello"))
            }
        }
        impl_scalar!($wrapper, schema::String);
    };
}

pub(crate) use create_from_str;
pub(crate) use create_id;

pub struct IdVisitor<V: From<u32>>(pub PhantomData<V>);

impl<'de, V: From<u32>> de::Visitor<'de> for IdVisitor<V> {
    type Value = V;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a number formatted a string")
    }
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(V::from(v.parse().map_err(|e| E::custom(format!("{e:?}")))?))
    }
}

pub struct FromStrVisitor<T> {
    expecting: &'static str,
    ty: PhantomData<T>,
}

impl<T> FromStrVisitor<T> {
    pub fn new(expecting: &'static str) -> Self {
        FromStrVisitor {
            expecting,
            ty: PhantomData,
        }
    }
}

impl<'de, T> Visitor<'de> for FromStrVisitor<T>
where
    T: str::FromStr,
    T::Err: fmt::Display,
{
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(self.expecting)
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        s.parse().map_err(Error::custom)
    }
}
