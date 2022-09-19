## `static-compress`

`static-compress` is command-line utility that can be used to aid in the generation of a statically pre-compressed copy of a given directory subtree, useful for serving statically precompressed content via a webserver like nginx or apache.

`static-compress` currently supports creating statically compressed copies of files matching a given glob (expression) in the gzip and brotli formats. gzip-compressed files can be generated either via the standard `miniz` library, or via the slower-but-higher-compression `zopfli` algorithm, [recently developed by Google](https://en.wikipedia.org/wiki/Zopfli). As of version 0.3, `static-compress` also features [webp](https://developers.google.com/speed/webp/) support for image compression.

### Installation

`static-compress` is available via the cargo package manager on all supported platforms and may be installed by simply executing

```bash
cargo install static-compress
```

Pre-built, signed binaries for select platforms can also be found at the static-compress homepage at
https://neosmart.net/static-compress/

### Usage

USAGE:

    static-compress [OPTIONS] <FILTER>...
Usage of `static-compress` is straightforward. It is invoked with either a list of files to pre-compress or an expression such as `"*.rs"` (to match all files in the current directory with a `.rs` extension) or `"**/*.png"` (to match `.png` files in all subdirectories).

No options are required, but optional command line switches are available to influence the behavior of `static-compress`:

    -c, --compressor <[brotli|gzip|            The compressor to use, defaulting to gzip
                       webp|zopfli]>
    -e, --extension <.EXT>                     The extension to use for compressed files. Supplied
                                               automatically if not provided.
    -j, --threads <COUNT>                      The number of simultaneous compressions.
    -i, case-insensitive                       Use case-insensitive matching against patterns.
    -q, --quality <QUALITY>                    The algorithm-specific quality parameter to be used.
                                               Automatically set if not provided.
    --quiet                                    Suppresses all non-error output.
    --no-progress                              Silences file progress information.
    --no-summary                               Suppress the end-of-run summary.

Multithreading may be achieved by means of the `-j` switch (akin to `make`), and can be used to specify the number of files to be compressed simultaneously across multiple threads. By default, `static-compress` uses all available threads.

The algorithm-specific `--quality` switch can be used to set the quality parameter for the chosen compressor (if supported):

| Compressor | `--quality` range |
| ---------- | ----------------- |
| gzip       | 0 - 10            |
| brotli     | 0 - 11            |
| webp       | 0 - 100           |
| zopfli     | *not supported*   |

### Supported Globs/Expressions

Supported filters/expressions include `*` to match any filename pattern, `**` to match recursively across all subdirectories, and `?` to substitute any single character. A bracket containing multiple characters will match any one character within the brackets (e.g. `[abc]` will match `a` or `b` but not `aa`), and curly braces can be used to match any of the comma-separated contents (e.g. `{abc,def}`).

`static-compress` supports the following filter expressions, all of which will match the same file `foo.bar`:

* `./relative/path/to/foo.bar`
* `/absolute/path/to/foo.bar`
* `relative/*/*/foo.ba?`
* `**/foo.bar`
* `**/foo.ba[rz]`
* `**/{foo|something}.bar`

**Important Note: Make sure to place expressions in double-quotes to prevent your shell from globbing the expressions!** i.e. use `static-compress "*.html"` and not `static-compress *.html`. The latter may cause an argument overflow in the presence of too many files, and will not use `static-compress`' intelligent globbing, relying on your shell to expand the glob instead!

### Supported Compression Methods

Currently, `static-compress` supports the `gzip` and `brotli` general-purpose compression algortithms for compressing web content. Almost all web servers and web browsers in use today have full `gzip` support. `brotli` is a newer web-compression format [developed by Google](https://en.wikipedia.org/wiki/Brotli), that can be used to achieve higher levels of compression than `gzip`, though compression is more taxing on the server. For that reason, it is especially desirable to be able to pre-compress a given directory tree instead of (re-)compressing files each time they are requested.

`static-compress` also supports zopfli, which is akin to `gzip -11` ([we jest!](https://www.youtube.com/watch?v=KOO5S4vxi0o)). The only problem is that `zopfli` is ridiculously slow and absolutely not intended to be used for dynamic compression. Again, this is another area where pre-compression is the way to go, and `static-compress` makes it easy to prepare a directory tree to serve zopfli-compressed versions of its contents. Unlike brotli, zopfli is gzip-compatible meaning any browser that supports gzip decompression also supports zopfli - but zopfli is both slower at compressing and typically does not achieve the same compression rates that brotli currently does. (Given the requirement of playing nicely with browsers from the 90s, it's good at what it does.)

As of version 0.3, `static-compress` also features webp support for image compression. In our testing on a corpus of the approximately 15,000 images uploaded to the NeoSmart website in PNG and JPG formats, webp compression with a quality parameter of 90 (the default `--quality` parameter for webp in `static-compress`) resulted in a 65% reduction in file size with no appreciable increase in artifacts.

### Mode of Operation

`static-compress` is an *intelligent* compressor meant for use in day-to-day web deployment and system administration tasks. The entire point of `static-compress` verses the usage of an extremely fragile and overly-complicated batch script (`find` with `mtime`, `gzip|brotli`, `parallel`, `touch`, and more) is to make life easier and the results more portable/deterministic. `static-compress` can be safely run against any directory tree, and by default it

* Compresses only files that haven't been previously statically compressed (it sets the modification date of the statically-compressed copy of a file to match the original, and only recompresses if this does not match),
* Does not compress already compressed files (i.e. won't recompress your pre-compressed `.gz` files as `.gz.br`),
* Can be configured to use as many or as few threads as you like for simultaneous compression,
* Can be used to compress an entire directory tree (`static-compress "**"`) or just files matching a certain extension (`static-compress "**/*.html"`) or only matching a certain prefix or subpath (`static-compress "**/tocompress/*"`)
* Sets the modification date of the compressed file equal to the modification date of the original file, so that when the original file is modified the webserver can know not to serve the old/stale compressed file (and so a subsequent `static-compress` run can know to re-compress the file and replace the stale copy).

### Web Server Configuration

Given a subdirectory `optimized`, the contents of which have been pre-compressed in both `gzip` and `brotli` formats via `static-compress optimized/**` and `static-compress optimized/** -c brotli`, the instructions for configuring your web server to use the statically pre-compressed version of the original files is as follows:

#### nginx:

To serve gzip-compressed files, nginx must be compiled with the `ngx_http_gzip_static_module` module (included in the default distribution) by specifying `--with-http_gzip_static_module` at build time. Thereafter, the following configuration may be used:

```nginx
location optimized {
  gzip_static on;
}
```

To serve brotli-compressed files, nginx must be compiled with the `ngx_brotli` module ([available separately](https://github.com/google/ngx_brotli)) by specifying `--add-module ../ngx_brotli` at build time. Thereafter, the following configuration may be used:

```nginx
location optimized {
  brotli_static on;
}
```

If both the `ngx_brotli` and `ngx_http_gzip_static_module` modules have been installed, the two directives may be safely used in the same `location` block:

```nginx
location optimized {
  brotli_static on;
  gzip_static on;
}
```

Note that the file type options for both modules (``brotli_types`` and `gzip_types`) do not apply to the static option; all files, even those not specified for dynamic compression via these two `_types` options, may be served in these formats if a `.br` or `.gz` file with the same name resides in the same directory.

### Acknowledgements, authorship, license, and copyright

`static-compress` is made freely available to the public under the terms of the MIT license. `static-compress` is open source and would not have been possible without the `flate2` and `zopfli` crate authors, as well as the original creators of the `brotli`, `gzip`, and `zopfli` algorithms.

`static-compress` is written by Mahmoud Al-Qudsi <[mqudsi@neosmart.net](mailto:mqudsi@neosmart.net)> under the stewardship of NeoSmart Technologies. The name `static-compress` and all other rights not conferred by the MIT license are reserved and copyright of NeoSmart Technologies, 2017-2022.
