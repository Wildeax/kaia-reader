/*
 * C shim over KOReader's MuPDF build. Written for kaia-mupdf-sys.
 *
 * Every function that calls MuPDF wraps the calls in fz_try/fz_catch. A MuPDF
 * exception never leaves this file: the function returns KAIA_ERR_MUPDF and the
 * message is available from kaia_doc_error(). No function calls back into Rust,
 * so setjmp/longjmp never crosses a Rust frame.
 *
 * Threading: a kaia_doc owns one fz_context. Use one kaia_doc from one thread at
 * a time. Different kaia_doc handles may run on different threads at once.
 * They share the process environment (FONTDIR is read when MuPDF loads a
 * fallback font) and one MuPDF global: fz_hb_lock() stores the active context
 * in the static fz_hb_secret. Only MuPDF's HarfBuzz allocator hooks
 * (fz_hb_malloc and friends) read it, and KOReader links the system HarfBuzz,
 * which does not call them, so concurrent handles do not depend on it.
 * MuPDF's color management library (lcms2mt) is thread safe.
 *
 * Ownership: pointers returned through out parameters are allocated with
 * malloc and must be released with kaia_free() or the matching *_free function.
 */
#ifndef KAIA_MUPDF_H
#define KAIA_MUPDF_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct kaia_doc kaia_doc;

enum kaia_status {
    KAIA_OK = 0,
    KAIA_ERR_MUPDF = 1,   /* MuPDF raised an exception; see kaia_doc_error() */
    KAIA_ERR_ARG = 2,     /* null pointer, empty area, bad scale, no document open */
    KAIA_ERR_NOMEM = 3,   /* malloc failed */
    KAIA_ERR_RANGE = 4    /* page number outside 0 .. count-1 */
};

/* Pixel data. format 0 is Gray8, format 1 is Rgb24. Rows are tightly packed
 * (stride == width * bytes per pixel), so stride is informational. */
typedef struct {
    int width;
    int height;
    int stride;
    int format;
    size_t len;
    unsigned char *data;
} kaia_pixels;

/* One outline entry, flattened depth first. level starts at 1. page is -1 when
 * the entry has no page target. title is never NULL. */
typedef struct {
    int level;
    int page;
    char *title;
} kaia_toc_item;

/* One link on a page. Box is in page units (points). page is the target page
 * for links that resolve inside the document, otherwise -1. uri is never NULL. */
typedef struct {
    float x0, y0, x1, y1;
    int page;
    char *uri;
} kaia_link;

/* Structured text, flattened. Blocks reference line ranges, lines reference
 * char ranges. Only text blocks (kind 0) have lines. kind 1 is an image block,
 * kind 2 is any other block type. */
typedef struct {
    int kind;
    float x0, y0, x1, y1;
    int first_line;
    int line_count;
} kaia_stext_block;

typedef struct {
    float x0, y0, x1, y1;
    int first_char;
    int char_count;
} kaia_stext_line;

typedef struct {
    int unicode;
    float x0, y0, x1, y1;
} kaia_stext_char;

typedef struct {
    int block_count;
    kaia_stext_block *blocks;
    int line_count;
    kaia_stext_line *lines;
    int char_count;
    kaia_stext_char *chars;
} kaia_stext;

/* Creates a handle with its own fz_context. Registers the document handlers and
 * the external font functions the way KOReader does. */
/* Trusted local HTML and CSS; fonts is a directory archive. Exceptions stay in C. */
int kaia_write_story_pdf(kaia_doc *d, const char *path, const char *html,
                         const char *css, const char *fonts);

int kaia_doc_new(kaia_doc **out);
void kaia_doc_free(kaia_doc *d);

/* Message for the last failed call on this handle. Empty string when none. */
const char *kaia_doc_error(const kaia_doc *d);

/* Opens a file, letting MuPDF pick the handler from the file name. */
int kaia_doc_open(kaia_doc *d, const char *path);

/* Opens a file with an explicit handler. magic is a file extension such as
 * ".epub" or a mime type. The file is read through a stream. */
int kaia_doc_open_as(kaia_doc *d, const char *path, const char *magic);

int kaia_doc_page_count(kaia_doc *d, int *out);
int kaia_doc_is_reflowable(kaia_doc *d, int *out);

/* Sets the user stylesheet used by HTML and EPUB layout. Call before layout. */
int kaia_doc_set_user_css(kaia_doc *d, const char *css);

/* Lays out a reflowable document for a page of width x height points at the
 * given em size. Fixed-layout documents return KAIA_ERR_MUPDF. */
int kaia_doc_layout(kaia_doc *d, float width, float height, float em);

/* Writes x0, y0, x1, y1 of the page box in points. */
int kaia_doc_page_bounds(kaia_doc *d, int page, float box[4]);

/* Renders a page at scale (1.0 is 72 dpi). clip, when not NULL, is x0, y0, x1,
 * y1 in scaled device pixels. When it is NULL the whole page is rendered. An
 * empty area, or one wider or taller than 20000 pixels, is an error.
 * gray is nonzero for Gray8 output and zero for Rgb24. */
int kaia_doc_render(kaia_doc *d, int page, float scale, const int *clip, int gray,
                    kaia_pixels *out);

/* Metadata by MuPDF key, for example "info:Title" or "info:Author". On success
 * *out is a malloc'd string, or NULL when the key is absent. */
int kaia_doc_metadata(kaia_doc *d, const char *key, char **out);

int kaia_doc_toc(kaia_doc *d, kaia_toc_item **items, int *count);
void kaia_toc_free(kaia_toc_item *items, int count);

int kaia_doc_links(kaia_doc *d, int page, kaia_link **links, int *count);
void kaia_links_free(kaia_link *links, int count);

int kaia_doc_stext(kaia_doc *d, int page, kaia_stext *out);
void kaia_stext_free(kaia_stext *out);

/* Searches a page. max_hits above 65536 is clamped to 65536. On success *quads
 * points to count * 8 floats in the order ul.x ul.y ur.x ur.y ll.x ll.y lr.x
 * lr.y. Free with kaia_free(). */
int kaia_doc_search(kaia_doc *d, int page, const char *needle, int max_hits,
                    float **quads, int *count);

/* Resolves an internal URI, such as "#chapter3", to a page number. *page is -1
 * when the URI does not point into the document. */
int kaia_doc_resolve_uri(kaia_doc *d, const char *uri, int *page);

/* Maps a MuPDF chapter and page location to a linear page number, or -1. */
int kaia_doc_location_page(kaia_doc *d, int chapter, int page, int *out);

void kaia_free(void *p);

#ifdef __cplusplus
}
#endif

#endif /* KAIA_MUPDF_H */
