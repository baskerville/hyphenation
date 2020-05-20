/*!
Methods for hyphenation dictionaries
*/

use std::borrow::Cow;

use kl_hyphenate_commons::dictionary::*;
use kl_hyphenate_commons::dictionary::extended::*;
use case_folding::{realign, refold, Shift};
use score::Score;


/// The indices of soft hyphens (U+00AD) within the string, if any. Existing
/// soft hyphens indicate a preferred hyphenation, which can be used without
/// resorting to best-effort hyphenation.
pub fn soft_hyphen_indices(word : &str) -> Option<Vec<usize>> {
    let shys : Vec<_> = word.match_indices('\u{00ad}').map(|(i, _)| i).collect();
    if shys.len() > 0 {
        Some(shys)
    } else { None }
}


/// A hyphenated word carrying valid breaks.
///
/// The `Word` can be borrowed or moved for iteration with `iter()` and
/// `into_iter()` respectively.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Word<'t, Break> {
    pub text : &'t str,
    pub breaks : Vec<Break>
}


/// A dictionary capable of hyphenating individual words.
///
/// For the purpose of hyphenation, a "word" should not be a compound in
/// hyphenated form (such as "hard-nosed"), but a single run of letters
/// without intervening punctuation or spaces.
///
/// For details, refer to the `patterns/*.chr.txt` file for each language.
pub trait Hyphenator<'h> {
    /// Plain representation of a word break.
    type Opportunity;

    /// An owned opportunity used to specify and store the predetermined hyphenation
    /// of known words.
    type Exact;


    /// Hyphenate a word, computing appropriate word breaks and preparing it for
    /// iteration.
    ///
    /// Soft hyphens take priority over dictionary hyphenation; if the word
    /// contains any, they will be returned as the only breaks available.
    ///
    /// This method is case-insensitive.
    fn hyphenate<'t>(&'h self, word : &'t str) -> Word<'t, Self::Opportunity>;

    /// The hyphenation opportunities that our dictionary can find in the given
    /// word. The word should be lowercase.
    fn opportunities(&'h self, lowercase_word : &str) -> Vec<Self::Opportunity> {
        match self.boundaries(lowercase_word) {
            None => vec![],
            Some(mins) => {
                match self.exact_within(lowercase_word, mins) {
                    None => self.opportunities_within(lowercase_word, mins),
                    Some(known) => known
                }
            }
        }
    }

    /// The hyphenation opportunities that arise between the specified indices.
    ///
    /// No attempt is made to retrieve a known exact hyphenation.
    fn opportunities_within(&'h self, lowercase_word : &str, bounds : (usize, usize))
        -> Vec<Self::Opportunity>;

    /// Retrieve the known exact hyphenation for this word, if any, between the specified indices.
    fn exact_within(&'h self, lowercase_word : &str, bounds : (usize, usize))
        -> Option<Vec<Self::Opportunity>>;

    /// Specify the hyphenation of the given word with an exact sequence of
    /// opportunities. Subsequent calls to `hyphenate` or `opportunities` will
    /// yield this hyphenation instead of generating one from patterns.
    ///
    /// If the word already has an exact hyphenation, the old opportunities
    /// are returned.
    fn add_exact(&mut self, word : String, ops : Vec<Self::Exact>) -> Option<Vec<Self::Exact>>;

    /// The number of `char`s from the start and end of a word where breaks may
    /// not occur.
    fn unbreakable_chars(&self) -> (usize, usize);

    /// The byte indices delimiting the substring where breaks may occur, unless
    /// the word is too short to be hyphenated.
    fn boundaries(&self, word : &str) -> Option<(usize, usize)> {
        let (l_min, r_min) = self.unbreakable_chars();
        let length_min = l_min + r_min;
        if word.chars().count() >= length_min {
            ( word.char_indices().nth(l_min).unwrap().0
            , word.char_indices().rev().nth(r_min.saturating_sub(1)).unwrap().0 ).into()
        } else { None }
    }
}


#[derive(Debug, Clone)]
struct Prepared<'t> {
    word : Cow<'t, str>,
    shifts : Vec<Shift>
}

fn prepare<'t>(text : &'t str) -> Prepared<'t> {
    let (word, shifts) = refold(text);
    Prepared { word, shifts }
}


impl<'h> Hyphenator<'h> for Standard {
    type Opportunity = usize;
    type Exact = usize;

    fn hyphenate<'t>(&'h self, word : &'t str) -> Word<'t, Self::Opportunity> {
        let breaks = match soft_hyphen_indices(word) {
            Some(ops) => ops,
            None => {
                let Prepared { ref word, ref shifts } = prepare(word);
                if shifts.len() > 0 {
                    self.opportunities(word).into_iter()
                        .map(move |o| realign(o, shifts)).collect()
                } else { self.opportunities(word) }
            }
        };

        Word { breaks, text : word }
    }

    fn opportunities_within(&'h self, word : &str, (l, r) : (usize, usize)) -> Vec<usize> {
        (1 .. word.len())
            .zip(self.score(word))
            .filter(|&(i, v)| {
                let valid = Self::denotes_opportunity(v);
                let within_bounds = i >= l && i <= r;
                let legal_index = word.is_char_boundary(i);
                valid && within_bounds && legal_index
            }).map(|(i, _)| i).collect()
    }

    #[inline]
    fn exact_within(&'h self, w : &str, (l, r) : (usize, usize)) -> Option<Vec<Self::Opportunity>> {
        self.exceptions.0.get(w).map(|v| v.iter().filter(|&i| *i >= l && *i <= r).cloned().collect())
    }

    #[inline]
    fn add_exact(&mut self, w : String, ops : Vec<usize>) -> Option<Vec<usize>> {
        self.exceptions.0.insert(w, ops)
    }

    #[inline] fn unbreakable_chars(&self) -> (usize, usize) { self.minima }
}

impl<'h> Hyphenator<'h> for Extended {
    type Opportunity = (usize, Option<&'h Subregion>);
    type Exact = (usize, Option<Subregion>);

    fn hyphenate<'t>(&'h self, word : &'t str) -> Word<'t, Self::Opportunity> {
        let breaks = match soft_hyphen_indices(word) {
            Some(ops) => ops.into_iter().map(|i| (i, None)).collect(),
            None => {
                let Prepared { ref word, ref shifts } = prepare(word);
                if shifts.len() > 0 {
                    self.opportunities(word).into_iter()
                        .map(move |(i, subr)| (realign(i, shifts), subr)).collect()
                } else { self.opportunities(word) }
            }
        };

        Word { breaks, text : word }
    }

    fn opportunities_within(&'h self, word : &str, (l, r) : (usize, usize))
        -> Vec<Self::Opportunity>
    {
        (1 .. word.len())
            .zip(self.score(word))
            .filter(|&(i, v)| {
                let valid = Self::denotes_opportunity(v);
                let within_bounds = i >= l && i <= r;
                let legal_index = word.is_char_boundary(i);
                valid && within_bounds && legal_index
            }).map(|(i, (_, subr))| (i, subr)).collect()
    }

    #[inline]
    fn exact_within(&'h self, w : &str, (l, r) : (usize, usize)) -> Option<Vec<Self::Opportunity>> {
        self.exceptions.0.get(w).map(|v| v.iter()
            .filter_map(|&(i, ref sub)| if i >= l && i <= r { Some((i, sub.as_ref())) } else { None }).collect())
    }

    #[inline]
    fn add_exact(&mut self, w : String, ops : Vec<Self::Exact>) -> Option<Vec<Self::Exact>> {
        self.exceptions.0.insert(w, ops)
    }

    #[inline] fn unbreakable_chars(&self) -> (usize, usize) { self.minima }
}
