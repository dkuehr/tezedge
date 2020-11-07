use crate::binary_reader::BinaryReaderError;
use nom::{
    IResult,
    Finish,
};


pub trait DeserializeNom: Sized {
    fn nom_parse<'a>(i: &'a [u8]) -> IResult<&'a [u8],Self>;
}

pub trait NomFrom<'a>: Sized {
    fn nom_from_bytes(i: &'a [u8]) -> Result<Self,BinaryReaderError>;
}

impl<'a, T> NomFrom<'a> for T where T: DeserializeNom {
    fn nom_from_bytes(i: &'a [u8]) -> Result<Self,BinaryReaderError> {
        match Self::nom_parse(i).finish() {
            Ok((_, res)) => Ok(res),
            Err(_) => Err(BinaryReaderError::NomDeserializationError),
        }
    }
}
