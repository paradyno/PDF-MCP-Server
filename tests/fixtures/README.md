# Test Fixtures

PDF files used for testing.

## Files

| File | Description | Source | License |
|------|-------------|--------|---------|
| `dummy.pdf` | Basic test document | [mupdf-rs](https://github.com/messense/mupdf-rs/tree/main/tests/files) | Apache-2.0/MIT |
| `dummy-encrypted.pdf` | Encrypted PDF (empty password) | [mupdf-rs](https://github.com/messense/mupdf-rs/tree/main/tests/files) | Apache-2.0/MIT |
| `basicapi.pdf` | PDF with outlines/bookmarks (3 pages) | [Mozilla pdf.js](https://github.com/mozilla/pdf.js/tree/master/test/pdfs) | Apache-2.0 |
| `tracemonkey.pdf` | Academic paper with images (14 pages, 90 images) | [Mozilla pdf.js](https://github.com/mozilla/pdf.js/tree/master/test/pdfs) | Apache-2.0 |
| `test-with-outline-and-images.pdf` | Custom PDF with hierarchical outlines AND embedded images (6 pages) | Generated | CC0-1.0 |

## Feature Matrix

| Feature | dummy | dummy-encrypted | basicapi | tracemonkey | test-with-outline-and-images |
|---------|-------|-----------------|----------|-------------|------------------------------|
| Multi-page | ❌ | ❌ | ✅ (3) | ✅ (14) | ✅ (6) |
| Outlines/Bookmarks | ✅ (1) | ❌ | ✅ (3) | ❌ | ✅ (6, hierarchical) |
| Embedded Images | ❌ | ❌ | ❌ | ✅ (90) | ✅ (4) |
| Password Protected | ❌ | ✅ | ❌ | ❌ | ❌ |

## Generating Custom Test PDF

To regenerate `test-with-outline-and-images.pdf`:

```bash
python3 -m venv .venv
.venv/bin/pip install reportlab pillow
.venv/bin/python create_test_pdf.py
rm -rf .venv
```

## License Details

### mupdf-rs files (`dummy.pdf`, `dummy-encrypted.pdf`)

These files are from the [mupdf-rs](https://github.com/messense/mupdf-rs) test suite.

- **License**: Dual-licensed under Apache-2.0 and MIT
- **Source**: https://github.com/messense/mupdf-rs/tree/main/tests/files

### Mozilla pdf.js files (`basicapi.pdf`, `tracemonkey.pdf`)

These files are from the [Mozilla pdf.js](https://github.com/mozilla/pdf.js) project.

- **License**: Apache-2.0
- **Source**: https://github.com/mozilla/pdf.js/tree/master/test/pdfs

### Generated files (`test-with-outline-and-images.pdf`)

This file was created specifically for this project using `create_test_pdf.py`.

- **License**: CC0-1.0 (Public Domain Dedication)
- **AGPL Compatibility**: ✅ CC0 works are in the public domain
- **Generator**: `create_test_pdf.py` (also CC0-1.0)

To the extent possible under law, the authors have waived all copyright and related or neighboring rights to `test-with-outline-and-images.pdf`. This work is published from Japan.

See: https://creativecommons.org/publicdomain/zero/1.0/

## Attribution Summary

```
dummy.pdf, dummy-encrypted.pdf
  Copyright (c) mupdf-rs contributors
  Licensed under Apache-2.0 OR MIT

basicapi.pdf, tracemonkey.pdf
  Copyright (c) Mozilla Foundation
  Licensed under Apache-2.0

test-with-outline-and-images.pdf
  CC0-1.0 Public Domain
```
