#![allow(dead_code)]

#[cfg(any(feature = "nfc", feature = "nfd", feature = "nfkc", feature = "nfkd"))]
extern crate unicode_normalization;

extern crate atlatl;
extern crate bincode;
extern crate hyphenation_commons;
extern crate serde;

use atlatl::fst;
use bincode as bin;
use serde::ser;
use std::collections::HashMap;
use std::hash::Hash;
use std::env;
use std::error;
use std::fmt;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};

use hyphenation_commons::dictionary::*;
use hyphenation_commons::dictionary::extended as ext;
use hyphenation_commons::Language;
use hyphenation_commons::parse::*;


// Configuration of exclusive optional features

use configuration::*;
mod configuration {
    // In service of configurable normalization forms, a type alias and a function
    // are defined via conditional compilation.
    //
    // If no feature is explicitly set, normalization is avoided altogether.

    // Neither Cargo nor rustc allows us to set exclusive features; we must indulge
    // them with this clumsy branle of cfg declarations.
    #[cfg(not(any(feature = "nfc", feature = "nfd", feature = "nfkc", feature = "nfkd")))]
    pub fn normalize(s : &str) -> String { s.to_owned() }

    #[cfg(any(feature = "nfc", feature = "nfd", feature = "nfkc", feature = "nfkd"))]
    use unicode_normalization::*;

    #[cfg(feature = "nfc")]  pub fn normalize(s : &str) -> String { s.nfc().collect() }
    #[cfg(feature = "nfd")]  pub fn normalize(s : &str) -> String { s.nfd().collect() }
    #[cfg(feature = "nfkc")] pub fn normalize(s : &str) -> String { s.nfkc().collect() }
    #[cfg(feature = "nfkd")] pub fn normalize(s : &str) -> String { s.nfkd().collect() }
}


trait TryFromIterator<Tally> : Sized {
    fn try_from_iter<I>(iter : I) -> Result<Self, Error>
    where I : IntoIterator<Item = (String, Tally)>
            + ExactSizeIterator;
}

fn uniques<I, T>(iter : I) -> (Vec<(String, u16)>, Vec<T>)
where T : Eq + Clone + Hash
    , I : IntoIterator<Item = (String, T)>
        + ExactSizeIterator
{
    let mut pairs = Vec::with_capacity(iter.len());
    let mut tally_ids = HashMap::with_capacity(iter.len());
    let mut tallies : Vec<T> = Vec::with_capacity(256);
    for (pattern, tally) in iter {
        match tally_ids.get(&tally) {
            Some(&id) => pairs.push((pattern, id)),
            None => {
                let id = tallies.len() as u16;
                tallies.push(tally.clone());
                tally_ids.insert(tally, id);
                pairs.push((pattern, id));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs.dedup_by(|a, b| a.0 == b.0);
    (pairs, tallies)
}

impl TryFromIterator<<Patterns as Parse>::Tally> for Patterns {
    fn try_from_iter<I>(iter : I) -> Result<Self, Error>
    where I : IntoIterator<Item = (String, <Patterns as Parse>::Tally)>
            + ExactSizeIterator
    {
        let (kvs, tallies) = uniques(iter);
        let builder = fst::Builder::from_iter(kvs.into_iter()) ?;
        let automaton : fst::FST<u32, u16> = fst::FST::from_builder(&builder) ?;
        Ok(Patterns {
            tallies : tallies,
            automaton : automaton
        })
    }
}

impl TryFromIterator<<Exceptions as Parse>::Tally> for Exceptions {
    fn try_from_iter<I>(iter : I) -> Result<Self, Error>
    where I : IntoIterator<Item = (String, <Exceptions as Parse>::Tally)>
            + ExactSizeIterator
    {
        Ok(Exceptions(HashMap::from_iter(iter)))
    }
}

impl TryFromIterator<<ext::Patterns as Parse>::Tally> for ext::Patterns {
    fn try_from_iter<I>(iter : I) -> Result<Self, Error>
    where I : IntoIterator<Item = (String, <ext::Patterns as Parse>::Tally)>
            + ExactSizeIterator
    {
        let (kvs, tallies) = uniques(iter);
        let builder = fst::Builder::from_iter(kvs.into_iter()) ?;
        let automaton : fst::FST<u32, u16> = fst::FST::from_builder(&builder) ?;
        Ok(ext::Patterns {
            tallies : tallies,
            automaton : automaton
        })
    }
}


// Dictionary building and serialization

#[derive(Clone, Debug)]
struct Paths {
    source : PathBuf,
    out : PathBuf
}

impl Paths {
    fn new() -> Result<Self, Error> {
        let source = env::var("CARGO_MANIFEST_DIR").map(|p| PathBuf::from(p)) ?;
        let out = source.clone();

        Ok(Paths { source, out })
    }

    fn dest_item<P : AsRef<Path>>(&self, p : P) -> PathBuf { self.out.join(p.as_ref()) }
    fn source_item<P : AsRef<Path>>(&self, p : P) -> PathBuf { self.source.join(p.as_ref()) }

    fn source_pattern(&self, lang : Language, suffix : &str) -> PathBuf {
        let fname = format!("hyph-{}.{}.txt", lang.code(), suffix);
        self.source_item("patterns").join(fname)
    }

    fn dest_dict(&self, lang : Language, suffix : &str) -> PathBuf {
        self.dest_item("dictionaries").join(Self::dict_name(lang, suffix))
    }

    fn dict_name(lang : Language, suffix : &str) -> String {
        format!("{}.{}.bincode", lang.code(), suffix)
    }
}


trait Build : Sized + Parse + TryFromIterator<<Self as Parse>::Tally> {
    fn suffix() -> &'static str;

    fn sourcepath(lang : Language, paths : &Paths) -> PathBuf {
        paths.source_pattern(lang, Self::suffix())
    }

    fn build(lang : Language, paths : &Paths) -> Result<Self, Error> {
        let file = File::open(Self::sourcepath(lang, paths)) ?;
        let by_line = io::BufReader::new(file).lines();
        let pairs : Vec<_> = by_line.map(|res| Self::pair(&res.unwrap(), normalize)).collect();

        Self::try_from_iter(pairs.into_iter())
    }
}

impl Build for Patterns   { fn suffix() -> &'static str { "pat" } }
impl Build for Exceptions { fn suffix() -> &'static str { "hyp" } }
impl Build for ext::Patterns { fn suffix() -> &'static str { "ext" } }


fn write<T>(item : &T, path : &Path) -> Result<(), Error> where T : ser::Serialize {
    let mut buffer = File::create(&path).map(|f| io::BufWriter::new(f)) ?;
    bin::config().limit(5_000_000).serialize_into(&mut buffer, item) ?;
    Ok(())
}


fn main() {
    #[cfg(feature = "build_dictionaries")]
    {
        use std::fs;
        use hyphenation_commons::Language::*;
        let _std_out = "standard";
        let _ext_out = "extended";
        let dict_folder = Path::new("dictionaries");
        let paths = Paths::new().unwrap();
        let dict_out = paths.dest_item(dict_folder);

        let ext_langs = vec![Catalan, Hungarian];
        let std_langs =
            vec![ Afrikaans, Armenian, Assamese, Basque, Belarusian, Bengali, Bulgarian, Catalan,
                  Chinese, Coptic, Croatian, Czech, Danish, Dutch, EnglishGB, EnglishUS, Esperanto,
                  Estonian, Ethiopic, Finnish, French, Friulan, Galician, Georgian, German1901,
                  German1996, GermanSwiss, GreekAncient, GreekMono, GreekPoly, Gujarati, Hindi,
                  Hungarian, Icelandic, Indonesian, Interlingua, Irish, Italian, Kannada, Kurmanji,
                  Latin, LatinClassic, LatinLiturgical, Latvian, Lithuanian, Macedonian, Malayalam,
                  Marathi, Mongolian, NorwegianBokmal, NorwegianNynorsk, Occitan, Oriya, Pali,
                  Panjabi, Piedmontese, Polish, Portuguese, Romanian, Romansh, Russian, Sanskrit,
                  SerbianCyrillic, SerbocroatianCyrillic, SerbocroatianLatin, SlavonicChurch, Slovak,
                  Slovenian, Spanish, Swedish, Tamil, Telugu, Thai, Turkish, Turkmen, Ukrainian,
                  Uppersorbian, Welsh ];

        fs::create_dir_all(&dict_out).unwrap();

        eprintln!("Building `Standard` dictionaries:");
        for &language in std_langs.iter() {
            eprintln!("{:?}", language);
            let dict = Standard {
                language,
                patterns : Patterns::build(language, &paths).unwrap(),
                exceptions : Exceptions::build(language, &paths).unwrap(),
                minima : language.minima()
            };

            write(&dict, &paths.dest_dict(language, _std_out)).unwrap();
        }

        eprintln!("Building `Extended` dictionaries:");
        for &language in ext_langs.iter() {
            eprintln!("{:?}", language);
            let dict = Extended {
                language,
                patterns : ext::Patterns::build(language, &paths).unwrap(),
                exceptions : ext::Exceptions::default(),
                minima : language.minima()
            };

            write(&dict, &paths.dest_dict(language, _ext_out)).unwrap();
        }
    }
}


// Error type boilerplate

#[derive(Debug)]
pub enum Error {
    Build(fst::Error),
    Env(env::VarError),
    IO(io::Error),
    Serialization(bin::Error),
    Resource
    // TODO: Parsing
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            Error::Build(ref e) => Some(e),
            Error::Env(ref e) => Some(e),
            Error::IO(ref e) => Some(e),
            Error::Serialization(ref e) => Some(e),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f : &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Build(ref e) => e.fmt(f),
            Error::Env(ref e) => e.fmt(f),
            Error::IO(ref e) => e.fmt(f),
            Error::Serialization(ref e) => e.fmt(f),
            Error::Resource => f.write_str("dictionary could not be embedded")
        }
    }
}

impl From<io::Error> for Error {
    fn from(err : io::Error) -> Error { Error::IO(err) }
}

impl From<env::VarError> for Error {
    fn from(err : env::VarError) -> Error { Error::Env(err) }
}

impl From<bin::Error> for Error {
    fn from(err : bin::Error) -> Error { Error::Serialization(err) }
}

impl From<fst::Error> for Error {
    fn from(err : fst::Error) -> Error { Error::Build(err) }
}
