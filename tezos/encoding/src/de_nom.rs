use crate::binary_reader::BinaryReaderError;
use nom::{
    IResult,
    Finish,
};

pub type NomInput<'a> = &'a [u8];
pub type NomResult<'a, T> = IResult<NomInput<'a>,T>;

pub trait NomDeserialize: Sized {
    fn nom_parse<'a>(i: NomInput<'a>) -> NomResult<'a, Self>;
}

pub trait NomFrom<'a>: Sized {
    fn nom_from_bytes(i: &'a [u8]) -> Result<Self,BinaryReaderError>;
}

impl<'a, T> NomFrom<'a> for T where T: NomDeserialize {
    fn nom_from_bytes(i: &'a [u8]) -> Result<Self,BinaryReaderError> {
        match Self::nom_parse(i).finish() {
            Ok((_, res)) => Ok(res),
            Err(_) => Err(BinaryReaderError::NomDeserializationError),
        }
    }
}
