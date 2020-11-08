use crate::binary_reader::BinaryReaderError;
use nom::{
    IResult,
    Finish,
    error::Error,
};

pub type NomInput<'a> = &'a [u8];
pub type NomResult<'a, O, E = Error<NomInput<'a>>> = IResult<NomInput<'a>, O, E>;

pub trait NomDeserialize: Sized {
    fn nom_parse<'a>(i: NomInput<'a>) -> NomResult<'a, Self>;
}

pub trait NomFrom: Sized {
    fn nom_from_bytes(i: Vec<u8>) -> Result<Self,BinaryReaderError>;
}

impl<T> NomFrom for T where T: NomDeserialize {
    fn nom_from_bytes(i: Vec<u8>) -> Result<Self,BinaryReaderError> {
        match Self::nom_parse(i.as_slice()).finish() {
            Ok((_, res)) => Ok(res),
            Err(_) => Err(BinaryReaderError::NomDeserializationError),
        }
    }
}

pub mod common {

    use super::*;

    pub use nom::{
        bytes::complete::{tag,take_till},
        number::complete::{i8, be_u32, be_u64},
        combinator::map,
        branch::alt,
        sequence::tuple,
    };
    
    use nom::{
        bytes::complete::take,
        multi::{length_value,many0},
        combinator::{all_consuming},
        sequence::preceded,
    };

    use crypto::hash::HashType;
    
    pub fn nom_dynamic<'a, O, E, F>(f: F) -> impl FnMut(NomInput<'a>) -> NomResult<'a, O, E>
    where
        F: nom::Parser<NomInput<'a>, O, E>,
        E: nom::error::ParseError<NomInput<'a>>
    {
        length_value(be_u32, all_consuming(f))
    }

    pub fn nom_list<'a, O, E, F>(f: F) -> impl FnMut(NomInput<'a>) -> NomResult<'a, Vec<O>, E>
    where
        F: nom::Parser<NomInput<'a>, O, E>,
        E: nom::error::ParseError<NomInput<'a>>
    {
        many0(f)
    }

    pub fn nom_tagged_enum<I, T, O1, O2, E: nom::error::ParseError<I>, F, M>(tag_item: T, func: F, mapper: M) -> impl FnMut(I) -> IResult<I, O2, E>
    where
        F: nom::Parser<I, O1, E>,
        M: FnMut(O1) -> O2,
        I: nom::InputTake + nom::Compare<T>,
        T: nom::InputLength + Clone,    
    {
        map(preceded(tag(tag_item), func), mapper)
    }

    pub fn nom_hash<'a, E>(t: HashType) -> impl FnMut(NomInput<'a>) -> NomResult<'a, Vec<u8>, E>
    where
        E: nom::error::ParseError<NomInput<'a>>
    {
        map(take(t.size()), |v| Vec::from(v))
    }

    pub fn nom_none<I, E: nom::error::ParseError<I>>() -> impl Fn(I) -> IResult<I, (), E> {
        |i| Ok((i, ()))
    }
}
