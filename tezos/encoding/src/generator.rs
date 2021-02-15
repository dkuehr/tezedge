use crate::encoding::{Encoding, Field, SchemaType::Binary};
use std::{ops::{RangeFrom, RangeInclusive}, time::Instant};

#[derive(Debug, Clone)]
pub struct Constraint {
    pub lower: Option<Idx>,
    pub upper: Option<Idx>,
}

impl Constraint {
    pub fn new(lower: Option<Idx>, upper: Option<Idx>) -> Constraint {
        Self { lower, upper }
    }

    #[inline]
    pub fn contains(&self, e: &Idx) -> bool {
        self.lower.map(|l| l <= *e).unwrap_or(true) &&
        self.upper.map(|u| *e <= u).unwrap_or(true)
    }
}

/*
impl<T> From<T> for Constraint where T: RangeBounds<Idx> {
    fn from(r: T) -> Self {
        let lower = match r.start_bound()
        Self::new(r.start_bound(), r.end_bound().map(|e| e - 1))
    }
}
 */

impl From<Range<Idx>> for Constraint {
    fn from(source: Range<Idx>) -> Self {
        Self::new(Some(source.start), Some(source.end - 1))
    }
}

impl From<RangeFrom<Idx>> for Constraint {
    fn from(source: RangeFrom<Idx>) -> Self {
        Self::new(Some(source.start), None)
    }
}

impl From<RangeInclusive<Idx>> for Constraint {
    fn from(source: RangeInclusive<Idx>) -> Self {
        Self::new(Some(*source.start()), Some(*source.end()))
    }
}

impl From<Idx> for Constraint {
    fn from(i: Idx) -> Self {
        Self::new(Some(i), Some(i))
    }
}

type Idx = usize;

pub trait IteratorFactory<T, I>
where
    I: Iterator<Item = T>,
{
    fn create(&self, id: &Path, r: &Constraint) -> I;
}

impl<F, T, P> IteratorFactory<P, <T as IntoIterator>::IntoIter> for F
where
    F: Fn(&Path, &Constraint) -> T,
    T: IntoIterator<Item = P>,
{
    fn create(&self, id: &Path, r: &Constraint) -> <T as IntoIterator>::IntoIter {
        (self)(id, r).into_iter()
    }
}

struct IterValue<T, I: Iterator<Item = T>> {
    iter: I,
    value: T,
}

impl<T: Clone, I> IterValue<T, I>
where
    I: Iterator<Item = T>,
{
    fn new(mut iter: I) -> Self {
        let value = iter
            .next()
            .expect("Iterator should yield at least one value");
        Self { iter, value }
    }

    fn value(&self) -> T {
        self.value.clone()
    }

    fn has_next(&mut self) -> bool {
        if let Some(next) = self.iter.next() {
            self.value = next;
            return true;
        } else {
            return false;
        }
    }
}

use std::{collections::BTreeMap, ops::Range};

pub struct IteratorContainer<T, F, I>
where
    T: Clone,
    F: IteratorFactory<T, I>,
    I: Iterator<Item = T>,
{
    factory: F,
    iters: BTreeMap<Path, IterValue<T, I>>,
    highest_updated: Option<usize>,
}

impl<T, F, I> IteratorContainer<T, F, I>
where
    T: Clone,
    F: IteratorFactory<T, I>,
    I: Iterator<Item = T>,
{
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            iters: BTreeMap::new(),
            highest_updated: None,
        }
    }

    pub fn get(&mut self, key: &Path, r: &Constraint) -> T {
        if !self.iters.contains_key(key) {
            self.iters
                .insert(key.clone(), IterValue::new(self.factory.create(&key, r)));
        }
        self.iters[key].value()
    }

    pub fn has_next(&mut self) -> bool {
        if self.iters.is_empty() {
            return true;
        }
        while let Some((key, mut iter_value)) = self.iters.pop_last() {
            self.highest_updated = self.highest_updated.map(|h| std::cmp::min(h, self.iters.len())).or(Some(self.iters.len()));
            if iter_value.has_next() {
                self.iters.insert(key, iter_value);
                return true;
            }
        }
        return false;
    }

    fn stat(&self) -> (usize, Option<usize>) {
        (self.iters.len(), self.highest_updated)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemKind {
    Root,
    Field(String),
    Index(Idx),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Path {
    path: String,
    kind: ItemKind,
}

impl Path {
    pub fn new(root: String) -> Path {
        Path { path: root, kind: ItemKind::Root }
    }

    fn field(&self, name: String) -> Path {
        Path {
            path: format!("{}.{}", self.path, name),
            kind: ItemKind::Field(name),
        }
    }

    fn index(&self, index: Idx) -> Path {
        Path {
            path: format!("{}[{}]", self.path, index),
            kind: ItemKind::Index(index)
        }
    }

    pub fn as_str(&self) -> &str {
        self.path.as_str()
    }

    pub fn get_field(&self) -> &str {
        self.path
            .rsplit('.')
            .next()
            .unwrap()
            .split('[')
            .next()
            .unwrap()
    }

    pub fn kind(&self) -> &ItemKind {
        &self.kind
    }
}

pub struct EncodingIter<'a, FI, FD, II, ID>
where
    FI: IteratorFactory<usize, II>,
    II: Iterator<Item = usize>,
    FD: IteratorFactory<(Vec<u8>, bool), ID>,
    ID: Iterator<Item = (Vec<u8>, bool)>,
{
    encoding: &'a Encoding,
    valid: bool,
    length_iters: IteratorContainer<usize, FI, II>,
    data_iters: IteratorContainer<(Vec<u8>, bool), FD, ID>,
    max: Vec<Idx>,
    t: Instant,
    c: usize,
}

impl<'a, FI, FD, II, ID> EncodingIter<'a, FI, FD, II, ID>
where
    FI: IteratorFactory<usize, II>,
    II: Iterator<Item = usize>,
    FD: IteratorFactory<(Vec<u8>, bool), ID>,
    ID: Iterator<Item = (Vec<u8>, bool)>,
{
    fn new(encoding: &'a Encoding, indices: FI, data: FD) -> Self {
        Self {
            encoding,
            valid: true,
            length_iters: IteratorContainer::new(indices),
            data_iters: IteratorContainer::new(data),
            max: Vec::new(),
            t: Instant::now(),
            c: 0,
        }
    }

    fn next_bounded<R: Into<Constraint>>(&mut self, path: &Path, r: R) -> Vec<u8> {
        let r = r.into();
        let (value, valid) = self.data_iters.get(path, &r);
        self.valid = self.valid && valid;
//        println!("value {} in {:?}..{:?}, valid: {} value: {:?}", path.as_str(), r.lower, r.upper, valid, value);
        value
    }
    fn next_length<R: Into<Constraint>>(&mut self, path: &Path, r: R) -> usize {
        let mut r = r.into();
        if r.upper.is_none() {
            if !self.max.is_empty() {
                using
            }
            r = Constraint::new(r.lower, self.max.last().map(|l| *l));
        }
        let value = self.length_iters.get(path, &r);
        let valid = r.contains(&value);
        self.valid = self.valid && valid;
//        println!("length of {} in {:?}..{:?}, valid: {} value: {}", path.as_str(), r.lower, r.upper, valid, value);
        value
    }

    fn extend_checked(&self, res: Option<Vec<u8>>, other: Option<Vec<u8>>) -> Option<Vec<u8>> {
        match (res, other, self.max.last()) {
            (Some(res), Some(other), Some(max)) if res.len() + other.len() > *max => None,
            (Some(mut res), Some(other), _) => {
                res.extend(other);
                Some(res)
            }
            _ => None,
        }
    }

    fn generate_field(&mut self, path: &Path, field: &Field, len: usize) -> Option<Vec<u8>> {
        self.generate(path.field(field.get_name().clone()), field.get_encoding(), len)
    }

    fn generate_element(
        &mut self,
        path: &Path,
        _index: Idx,
        encoding: &Encoding,
        len: usize,
    ) -> Option<Vec<u8>> {
        self.generate(path.index(0), encoding, len)
    }

    fn generate_length(len: Idx) -> Vec<u8> {
        (len as u32).to_be_bytes().to_vec()
    }

    fn generate(&mut self, path: Path, encoding: &Encoding, len: usize) -> Option<Vec<u8>> {
        match encoding {
            Encoding::Int8 | Encoding::Uint8 => Some(self.next_bounded(&path, 1)),
            Encoding::Int16 | Encoding::Uint16 => Some(self.next_bounded(&path, 2)),
            Encoding::Int32 | Encoding::Uint32 => Some(self.next_bounded(&path, 4)),
            Encoding::Int64 | Encoding::Timestamp => Some(self.next_bounded(&path, 8)),
            Encoding::Hash(hash_type) => {
                Some(self.next_bounded(&path, hash_type.size()))
            }
            Encoding::Obj(fields) => fields.iter().fold(Some(Vec::new()), |res, f| {
                let fld = self.generate_field(&path, f, len + res.len());
                self.extend_checked(res, fld)
            }),
            Encoding::List(encoding) => {
                let length = self.next_length(&path, 0..);
                (0..length).fold(Some(Vec::new()), |res, i| {
                    let elt = self.generate_element(&path, i, encoding, len + res.len());
                    self.extend_checked(res, elt)
                })
            }
            Encoding::BoundedList(max, encoding) => {
                let length = self.next_length(&path, 0..=*max);
                (0..length).fold(Some(Vec::new()), |res, i| {
                    let elt = self.generate_element(&path, i, encoding, len + res.len());
                    self.extend_checked(res, elt)
                })
            }
            Encoding::BoundedString(max) => {
                let s = self.next_bounded(&path, 0..=*max);
                let mut vec = (s.len() as u32).to_be_bytes().to_vec();
                vec.extend(s);
                Some(vec)
            }
            Encoding::Dynamic(encoding) => {
                self.generate(path, encoding, len).map(|res| {
                    let mut r = Self::generate_length(res.len());
                    r.extend(res);
                    r
                })
            }
            Encoding::Bounded(max, encoding) => {
                self.max.push(*max);
                let res = self.generate(path, encoding, len);
                self.max.pop();
                res
            }
            Encoding::Split(inner) => self.generate(path, &inner(Binary), len),
            _ => {
                unimplemented!("Encoding {:?} is not implemented", encoding);
            }
        }
    }
}

impl<'a, FI, FD, II, ID> Iterator for EncodingIter<'a, FI, FD, II, ID>
where
    FI: IteratorFactory<usize, II>,
    II: Iterator<Item = usize>,
    FD: IteratorFactory<(Vec<u8>, bool), ID>,
    ID: Iterator<Item = (Vec<u8>, bool)>,
{
    type Item = (Vec<u8>, bool);

    fn next(&mut self) -> Option<Self::Item> {
        use std::time::*;
        while self.length_iters.has_next() || self.data_iters.has_next() {
            if self.t.elapsed().as_secs() > 1 {
                let ((cd, hd), (cl, hl)) = (self.data_iters.stat(), self.length_iters.stat());
                let hd = hd.unwrap_or(cd);
                let hl = hl.unwrap_or(cl);
                println!("Updated {} of {}, {} ops per sec", hd + hl, cd + cl, self.c);
                self.t = Instant::now();
                self.c = 0;
            }
            self.valid = true;
//            println!("---------------------------------");
            if let Some(encoded) = self.generate(Path::new("root".to_string()), self.encoding) {
                self.c += 1;
//                println!("done: {}", self.valid);
                return Some((encoded, self.valid));
            }
        }
        None
    }
}

pub fn iter<'a, FI, FD, II, ID>(
    encoding: &'a Encoding,
    indices: FI,
    datas: FD,
) -> EncodingIter<'a, FI, FD, II, ID>
where
    FI: IteratorFactory<usize, II>,
    II: Iterator<Item = usize>,
    FD: IteratorFactory<(Vec<u8>, bool), ID>,
    ID: Iterator<Item = (Vec<u8>, bool)>,
{
    EncodingIter::new(encoding, indices, datas)
}

pub fn range_simple(r: &Constraint) -> std::vec::IntoIter<Idx> {
    let start = r.lower.unwrap_or(0);
    let end = r.upper.unwrap_or(Idx::MAX);
    let vec = vec![start, end];
    vec.into_iter()
}

pub fn range_extended(r: &Constraint) -> std::vec::IntoIter<Idx> {
    let start = r.lower.unwrap_or(0);
    let end = r.upper.unwrap_or(Idx::MAX);
    let mut vec = vec![start, end];
    if start > 0 {
        vec.push(start - 1);
    }
    if end < Idx::MAX {
        vec.push(end + 1);
    }
    vec.into_iter()
}

#[cfg(test)]
mod test {
    use crate::encoding::Field;

    use super::*;

    fn test_indices(_p: &Path, r: &Constraint) -> impl Iterator<Item = usize> {
        range_extended(r)
    }

    fn test_data(p: &Path, r: &Constraint) -> impl Iterator<Item = (Vec<u8>, bool)> {
        let it = range_extended(r);
        let p = p.clone();
        let r = r.clone();
        it.map(move |i| {
            let valid = r.contains(&i);
            let data = p
                .get_field()
                .as_bytes()
                .to_vec()
                .into_iter()
                .cycle()
                .take(i)
                .collect::<Vec<u8>>();
            (data, valid)
        })
    }

    #[test]
    fn test_generator() {
        let encoding = Encoding::Obj(vec![Field::new(
            "id",
            Encoding::bounded_list(10, Encoding::BoundedString(10)),
        )]);

        let _it = iter(&encoding, test_indices, test_data).for_each(|(_, v)| {
            println!("valid: {}", v);
        });
    }
}
