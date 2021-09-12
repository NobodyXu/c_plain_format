use std::convert::TryInto;

use serde::Deserialize;
use serde::de::{
    self, DeserializeSeed, EnumAccess, IntoDeserializer, SeqAccess,
    VariantAccess, Visitor,
};

use crate::{Error, Result};

#[derive(Debug)]
pub struct Deserializer<'de> {
    input: &'de [u8],
}

impl<'de> Deserializer<'de> {
    pub fn from_bytes(input: &'de [u8]) -> Self {
        Deserializer { input }
    }
}

pub fn from_bytes<'a, T>(s: &'a [u8]) -> Result<T>
where
    T: Deserialize<'a>,
{
    let mut deserializer = Deserializer::from_bytes(s);
    let t = T::deserialize(&mut deserializer)?;
    if deserializer.input.is_empty() {
        Ok(t)
    } else {
        Err(Error::TrailingBytes)
    }
}

impl<'de> Deserializer<'de> {
    fn peek_byte(&mut self) -> Result<u8> {
        self.input.iter().next().ok_or(Error::Eof).map(|byte| *byte)
    }

    fn next_byte(&mut self) -> Result<u8> {
        let ch = self.peek_byte()?;
        self.input = &self.input[1..];
        Ok(ch)
    }

    fn next_bytes_const<const SIZE: usize>(&mut self) -> Result<[u8; SIZE]> {
        let bytes: [u8; SIZE] = 
            self.input[..SIZE]
                .try_into()
                .map_err(|_| Error::Eof)?;
        self.input = &self.input[SIZE..];
        Ok(bytes)
    }

    fn next_bytes(&mut self, size: usize) -> Result<&'de [u8]> {
        if self.input.len() > size {
            Err(Error::Eof)
        } else {
            let bytes = &self.input[..size];
            self.input = &self.input[size..];
            Ok(bytes)
        }
    }

    /// Parse &str and &[u8]
    fn parse_bytes(&mut self) -> Result<&'de [u8]> {
        let len = self.next_u32()? as usize;
        self.next_bytes(len)
    }

    fn next_u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.next_bytes_const()?))
    }
}

macro_rules! impl_for_deserialize_primitive {
    ( $name:ident, $visitor_fname:ident, $type:ty ) => {
        fn $name<V>(self, visitor: V) -> Result<V::Value>
        where
            V: Visitor<'de>,
        {
            visitor.$visitor_fname(<$type>::from_be_bytes(self.next_bytes_const()?))
        }
    }
}

impl<'de, 'a> de::Deserializer<'de> for &'a mut Deserializer<'de> {
    type Error = Error;

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.next_u32()? {
            1 => visitor.visit_bool(true),
            0 => visitor.visit_bool(false),
            _ => Err(Error::InvalidBoolEncoding),
        }
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u8(self.next_byte()?)
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_i8(self.next_byte()? as i8)
    }

    impl_for_deserialize_primitive!(deserialize_i16, visit_i16, i16);
    impl_for_deserialize_primitive!(deserialize_i32, visit_i32, i32);
    impl_for_deserialize_primitive!(deserialize_i64, visit_i64, i64);

    impl_for_deserialize_primitive!(deserialize_u16, visit_u16, u16);
    impl_for_deserialize_primitive!(deserialize_u32, visit_u32, u32);
    impl_for_deserialize_primitive!(deserialize_u64, visit_u64, u64);

    impl_for_deserialize_primitive!(deserialize_f32, visit_f32, f32);
    impl_for_deserialize_primitive!(deserialize_f64, visit_f64, f64);

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match char::from_u32(self.next_u32()?) {
            Some(ch) => visitor.visit_char(ch),
            None => Err(Error::InvalidChar),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match std::str::from_utf8(self.parse_bytes()?) {
            Ok(s) => visitor.visit_borrowed_str(s),
            Err(e) => Err(Error::InvalidStr(e)),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_borrowed_bytes(self.parse_bytes()?)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        struct Access<'a, 'de> {
            deserializer: &'a mut Deserializer<'de>,
            len: usize,
        }

        impl<'a, 'de> SeqAccess<'de> for Access<'a, 'de>
        {
            type Error = Error;

            fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
            where
                T: DeserializeSeed<'de>,
            {
                if self.len > 0 {
                    self.len -= 1;
                    let value = seed.deserialize(&mut *self.deserializer)?;
                    Ok(Some(value))
                } else {
                    Ok(None)
                }
            }

            fn size_hint(&self) -> Option<usize> {
                Some(self.len)
            }
        }

        visitor.visit_seq(Access {
            deserializer: self,
            len,
        })
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(fields.len(), visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        impl<'a, 'de> EnumAccess<'de> for &'a mut Deserializer<'de>
        {
            type Error = Error;
            type Variant = Self;

            fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
            where
                V: DeserializeSeed<'de>,
            {
                let idx = self.next_u32()?;
                let val: Result<_> = seed.deserialize(idx.into_deserializer());
                Ok((val?, self))
            }
        }

        visitor.visit_enum(self)
    }

    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_seq<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::Unsupported("deserialize_seq"))
    }

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::Unsupported("deserialize_any"))
    }

    fn deserialize_option<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::Unsupported("deserialize_option"))
    }

    fn deserialize_map<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::Unsupported("deserialize_map"))
    }

    fn deserialize_identifier<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::Unsupported("deserialize_identifier"))
    }

    fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::Unsupported("deserialize_ignored_any"))
    }
}


impl<'a, 'de> VariantAccess<'de> for &'a mut Deserializer<'de>
{
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(self)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_tuple(self, len, visitor)
    }

    fn struct_variant<V>(self, fields: &'static [&'static str], visitor: V)
        -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_tuple(self, fields.len(), visitor)
    }
}
