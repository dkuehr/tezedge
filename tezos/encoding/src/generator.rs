use crate::encoding::Encoding;

/*
pub struct Constraints(Range);

impl Constraints {
    #[inline]
    fn start(&self) -> Idx {
        self.0.start;
    }

    #[inline]
    fn end(&self) -> Idx {
        self.0.end
    }
}

impl From<Idx> for Constraints {
    fn from(i: Idx) -> Self {
        Self(i..i+1)
    }
}

impl From<Range<Idx>> for Constraints {
    fn from(r: Range<Idx>) -> Constraints {
        Self(r)
    }
}

impl From<>

 */



type Idx = usize;

pub trait IteratorFactory<T, I>
where I : Iterator<Item = T>
{
    fn create(&self, id: &Path, r: Range<Idx>) -> I;
}

impl<F, T, P> IteratorFactory<P, <T as IntoIterator>::IntoIter> for F
where F: Fn(&Path, Range<Idx>) -> T,
      T: IntoIterator<Item = P>
{
    fn create(&self, id: &Path, r: Range<Idx>) -> <T as IntoIterator>::IntoIter {
        (self)(id, r).into_iter()
    }
}

struct IterValue<T, I: Iterator<Item = T>> {
    iter: I,
    value: T,
}

impl<T: Clone, I> IterValue<T, I>
where I: Iterator<Item = T>
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
where T: Clone,
      F: IteratorFactory<T, I>,
      I: Iterator<Item = T>
{
    factory: F,
    iters: BTreeMap<Path, IterValue<T, I>>,
}

impl<T, F, I> IteratorContainer<T, F, I>
where T: Clone,
      F: IteratorFactory<T, I>,
      I: Iterator<Item = T>
{
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            iters: BTreeMap::new(),
        }
    }

    pub fn get(&mut self, key: &Path, r: Range<Idx>) -> T {
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
            if iter_value.has_next() {
                self.iters.insert(key, iter_value);
                return true;
            }
        }
        return false;
    }
}

pub trait DataProvider {
    fn next_bounded(&mut self, path: &Path, r: Range<Idx>) -> Vec<u8>;
    fn next_length(&mut self, path: &Path, r: Range<Idx>) -> usize;
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Path {
    path: String,
}

impl Path {
    pub fn new(root: String) -> Path {
        Path { path: root }
    }

    fn field(&self, name: String) -> Path {
        Path {
            path: format!("{}.{}", self.path, name),
        }
    }

    fn index(&self, index: String) -> Path {
        Path {
            path: format!("{}[{}]", self.path, index),
        }
    }

    pub fn as_str(&self) -> &str {
        self.path.as_str()
    }

    pub fn get_field(&self) -> &str {
        self.path.rsplit('.').next().unwrap().split('[').next().unwrap()
    }
}

pub struct EncodingIter<'a, FI, FD, II, ID>
where FI: IteratorFactory<usize, II>,
      II: Iterator<Item = usize>,
      FD: IteratorFactory<(Vec<u8>, bool), ID>,
      ID: Iterator<Item = (Vec<u8>, bool)>
{
    encoding: &'a Encoding,
    valid: bool,
    length_iters: IteratorContainer<usize, FI, II>,
    data_iters: IteratorContainer<(Vec<u8>, bool), FD, ID>,
}

impl<'a, FI, FD, II, ID> EncodingIter<'a, FI, FD, II, ID>
where FI: IteratorFactory<usize, II>,
      II: Iterator<Item = usize>,
      FD: IteratorFactory<(Vec<u8>, bool), ID>,
      ID: Iterator<Item = (Vec<u8>, bool)>
{
    fn new(encoding: &'a Encoding, indices: FI, data: FD) -> Self {
        Self { encoding, valid: true, length_iters: IteratorContainer::new(indices), data_iters: IteratorContainer::new(data) }
    }
}

impl<'a, FI, FD, II, ID> Iterator for EncodingIter<'a, FI, FD, II, ID>
where FI: IteratorFactory<usize, II>,
      II: Iterator<Item = usize>,
      FD: IteratorFactory<(Vec<u8>, bool), ID>,
      ID: Iterator<Item = (Vec<u8>, bool)>
{
    type Item = (Vec<u8>, bool);

    fn next(&mut self) -> Option<Self::Item> {
        if self.length_iters.has_next() || self.data_iters.has_next() {
            self.valid = true;
            //println!("---------------------------------");
            let encoded = generate(Path::new("root".to_string()), self.encoding, self);
            //println!("done: {}", self.valid);
            Some((encoded, self.valid))
        } else {
            None
        }

    }
}

impl<'a, FI, FD, II, ID> DataProvider for EncodingIter<'a, FI, FD, II, ID>
where FI: IteratorFactory<usize, II>,
      II: Iterator<Item = usize>,
      FD: IteratorFactory<(Vec<u8>, bool), ID>,
      ID: Iterator<Item = (Vec<u8>, bool)>
{
    fn next_bounded(&mut self, path: &Path, r: Range<Idx>) -> Vec<u8> {
        let (value, valid) = self.data_iters.get(path, r.clone());
        self.valid = self.valid && valid;
        //println!("value {} in {}..{}, valid: {} value: {:?}", path.as_str(), r.start, r.end, valid, value);
        value
    }
    fn next_length(&mut self, path: &Path, r: Range<Idx>) -> usize {
        let value = self.length_iters.get(path, r.clone());
        let valid = r.contains(&value);
        self.valid = self.valid && valid;
        //println!("length of {} in {}..{}, valid: {} value: {}", path.as_str(), r.start, r.end, valid, value);
        value
    }
}

pub fn iter<'a, FI, FD, II, ID>(encoding: &'a Encoding, indices: FI, datas: FD) -> EncodingIter<'a, FI, FD, II, ID>
where FI: IteratorFactory<usize, II>,
      II: Iterator<Item = usize>,
      FD: IteratorFactory<(Vec<u8>, bool), ID>,
      ID: Iterator<Item = (Vec<u8>, bool)>
{
    EncodingIter::new(encoding, indices, datas)
}

fn generate(path: Path, encoding: &Encoding, data_generator: &mut dyn DataProvider) -> Vec<u8> {
    match encoding {
        Encoding::Uint8 => data_generator.next_bounded(&path, 1..2),
        Encoding::Int8 => data_generator.next_bounded(&path, 1..2),
        Encoding::Uint16 => data_generator.next_bounded(&path, 2..3),
        Encoding::Int16 => data_generator.next_bounded(&path, 2..3),
        Encoding::Obj(fields) => fields
            .iter()
            .map(|field| {
                generate(
                    path.field(field.get_name().clone()),
                    field.get_encoding(),
                    data_generator,
                )
            })
            .flatten()
            .collect(),
        Encoding::BoundedList(max, encoding) => {
            let length = data_generator.next_length(&path, 0..*max + 1);
            (0..length)
                .map(|index| generate(path.index(index.to_string()), encoding, data_generator))
                .flatten()
                .collect()
        }
        Encoding::BoundedString(max) => {
            let s = data_generator.next_bounded(&path, 0..*max + 1);
            let mut vec = (s.len() as u32).to_be_bytes().to_vec();
            vec.extend(s);
            vec
        }
        _ => {
            unimplemented!();
        }
    }
}

pub fn range_simple(r: Range<Idx>) -> impl Iterator<Item = Idx> {
    let vec = vec![r.start, r.end - 1];
    vec.into_iter()
}

pub fn range_extended(r: Range<Idx>) -> impl Iterator<Item = Idx> {
    let mut vec = vec![r.start, r.end - 1];
    if r.start > 0 {
        vec.push(r.start - 1);
    }
    vec.push(r.end);
    vec.into_iter()
}

#[cfg(test)]
mod test {
    use crate::encoding::Field;

    use super::*;

    #[test]
    fn test_list_of_strings() {
        let encoding = Encoding::Obj(vec![Field::new(
            "id",
            Encoding::bounded_list(10, Encoding::BoundedString(10)),
        )]);

        struct TestDataGenerator {}
        impl DataProvider for TestDataGenerator {
            fn next_bounded(&mut self, _: &Path, r: Range<Idx>) -> Vec<u8> {
                let mut vec = Vec::new();
                vec.resize(r.end - r.start, 0);
                vec
            }
            fn next_length(&mut self, _: &Path, r: Range<Idx>) -> usize {
                r.end - 1
            }
        }

        let encoded = generate(
            Path::new("AdvertiseMessage".to_string()),
            &encoding,
            &mut TestDataGenerator {},
        );

        assert_eq!(encoded.len(), 10 * (4 + 10));
    }

    fn test_indices(_p: &Path, r: Range<Idx>) -> impl Iterator<Item = usize> {
        range_extended(r)
    }

    fn test_data(p: &Path, r: Range<Idx>) -> impl Iterator<Item = (Vec<u8>, bool)> {
        let it = range_extended(r.clone());
        let p = p.clone();
        it.map(move |i| {
            let valid = r.contains(&i);
            let data = p.get_field().as_bytes().to_vec().into_iter().cycle().take(i).collect::<Vec<u8>>();
            (data, valid)
        })
    }

    #[test]
    fn test_test() {
        let encoding = Encoding::Obj(vec![Field::new(
            "id",
            Encoding::bounded_list(10, Encoding::BoundedString(10)),
        )]);

        let _it = iter(&encoding, test_indices, test_data).for_each(|(d, v)| {
            println!("valid: {}", v);
        });
    }

}
