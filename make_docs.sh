#!/bin/sh

set -e

rustdoc src/lib.rs
cp assets/doc_style.css doc/main.css
cd doc
rm *.woff
