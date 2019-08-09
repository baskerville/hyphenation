## Introduction

Two strategies are available:
- Standard Knuth–Liang hyphenation, with dictionaries built from the [TeX UTF-8 patterns](http://www.ctan.org/tex-archive/language/hyph-utf8).
- Extended (“non-standard”) hyphenation based on László Németh's [Automatic non-standard hyphenation in OpenOffice.org](https://www.tug.org/TUGboat/tb27-1/tb86nemeth.pdf), with dictionaries built from Libre/OpenOffice patterns.

## Usage

### Quickstart

The dictionaries can be built with:
```shell
cargo build -vv --features build_dictionaries
```
The resulting dictionaries are saved in the `dictionaries` directory.

You can then load and use a dictionary with:
```rust
let path_to_dict = "dictionaries/en-us.standard.bincode";
let en_us = Standard::from_path(Language::EnglishUS, path_to_dict) ?;

// Identify valid breaks in the given word.
let hyphenated = en_us.hyphenate("hyphenation");

// Word breaks are represented as byte indices into the string.
let break_indices = &hyphenated.breaks;
assert_eq!(break_indices, &[2, 6]);

// The segments of a hyphenated word can be iterated over.
let segments = hyphenated.into_iter();
let collected : Vec<String> = segments.collect();
assert_eq!(collected, vec!["hy", "phen", "ation"]);

/// `hyphenate()` is case-insensitive.
let uppercase : Vec<_> = en_us.hyphenate("CAPITAL").into_iter().collect();
assert_eq!(uppercase, vec!["CAP", "I", "TAL"]);
```

### Segmentation

Dictionaries can be used in conjunction with text segmentation to hyphenate words within a text run. This short example uses the [`unicode-segmentation`](https://crates.io/crates/unicode-segmentation) crate for untailored Unicode segmentation.

```rust
use unicode_segmentation::UnicodeSegmentation;

let hyphenate_text = |text : &str| -> String {
    // Split the text on word boundaries—
    text.split_word_bounds()
        // —and hyphenate each word individually.
        .flat_map(|word| en_us.hyphenate(word).into_iter())
        .collect()
};

let excerpt = "I know noble accents / And lucid, inescapable rhythms; […]";
assert_eq!("I know no-ble ac-cents / And lu-cid, in-escapable rhythms; […]"
          , hyphenate_text(excerpt));
```

### Normalization

Hyphenation patterns for languages affected by normalization occasionally cover multiple forms, at the discretion of their authors, but most often they don’t. If you require `kl-hyphenate` to operate strictly on strings in a known normalization form, as described by the [Unicode Standard Annex #15](http://unicode.org/reports/tr15/) and provided by the [`unicode-normalization`](https://github.com/unicode-rs/unicode-normalization) crate, you may specify it in your Cargo manifest, like so:

```toml
[dependencies.kl-hyphenate]
version = "…"
features = ["nfc"]
```

The `features` field may contain exactly *one* of the following normalization options:

- `"nfc"`, for canonical composition;
- `"nfd"`, for canonical decomposition;
- `"nfkc"`, for compatibility composition;
- `"nfkd"`, for compatibility decomposition.

It is recommended to build `kl-hyphenate` in release mode if normalization is enabled, since the bundled hyphenation patterns will need to be reprocessed into dictionaries.

## License

Dual-licensed under the terms of either:
  - the Apache License, Version 2.0
  - the MIT license

`texhyphen` and other hyphenation patterns © their respective owners; see `patterns/*.lic.txt` files for licensing information.
