/*
 * C ABI over KOReader's CREngine build. Every function returns KAIA_CRE_OK,
 * KAIA_CRE_NOT_FOUND or KAIA_CRE_ERROR. No C++ exception crosses this line.
 * On KAIA_CRE_ERROR, kaia_cre_last_error() returns the message for the
 * calling thread.
 *
 * Engine calls are serialized. Live handles are limited to leave room in
 * CREngine's fixed DOM table for temporary documents. doc_new waits for a
 * slot. If no handle closes for 10 seconds it returns KAIA_CRE_ERROR with a
 * capacity message, which is what a thread that holds every slot gets.
 * Callers must not hold locks needed by other document owners across doc_new.
 * Keep each handle on its creating thread.
 *
 * Pages are 0-based. Coordinates are device pixels of the view. Strings are
 * UTF-8 and owned by the caller once returned: free them with kaia_cre_free.
 */
#ifndef KAIA_CRE_H
#define KAIA_CRE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define KAIA_CRE_OK 0
#define KAIA_CRE_NOT_FOUND 1
#define KAIA_CRE_ERROR (-1)

typedef struct kaia_cre_doc kaia_cre_doc;

/* One character inside the page window. `line` counts visual lines from 0. */
typedef struct {
    float x0, y0, x1, y1;
    int32_t line;
    uint32_t codepoint;
} kaia_cre_char_box;

/* A flattened table of contents entry. `page` is -1 when the entry has no page. */
typedef struct {
    int32_t level;
    int32_t page;
    char *title;
    char *xpointer;
} kaia_cre_toc_entry;

/* A search hit as two xpointers. */
typedef struct {
    char *start;
    char *end;
} kaia_cre_range;

/* KOReader findAllText fields, in the order documented by kaia_cre_find_all. */
typedef struct { char *fields[9]; } kaia_cre_text_match;
int kaia_cre_find_all(kaia_cre_doc *doc, const char *pattern, int insensitive, int regex,
    int context, int max_hits, kaia_cre_text_match **out, size_t *count);
void kaia_cre_free_text_matches(kaia_cre_text_match *items, size_t count);
int kaia_cre_text_range(kaia_cre_doc *doc, const char *start, const char *end, char **out);
int kaia_cre_word_boundary(kaia_cre_doc *doc, const char *xp, int next, char **out);

/* One rectangle of a link. A link that spans lines has one box per line.
 * `page` is the target page for an internal link that resolves, or -1.
 * `href` is the link target as written in the book. */
typedef struct {
    float x0, y0, x1, y1;
    int32_t page;
    char *href;
    char *xpointer;
    char *note;
} kaia_cre_link_box;

const char *kaia_cre_last_error(void);

/* Registers font files and sets the hyphenation dictionary directory. Each
 * call can add fonts. `registered` counts usable supplied files. Zero is an error. */
int kaia_cre_init(const char *const *font_files, size_t font_count, const char *hyph_dir, int *registered);

/* Uses <data_dir>/hyph. Font files are discovered by the Rust runtime options. */
int kaia_cre_init_with_data(const char *const *font_files, size_t font_count, const char *data_dir, int *registered);

/* A font family CREngine has registered, with the file it reads for regular text. */
typedef struct {
    char *family;
    char *file;
} kaia_cre_font_family;

/* Lists the registered font families sorted by name, as FreeType reports them.
 * Free the array with kaia_cre_free_font_families. Needs an initialized engine. */
int kaia_cre_font_families(kaia_cre_font_family **out, size_t *count);
void kaia_cre_free_font_families(kaia_cre_font_family *families, size_t count);

/* Fails with KAIA_CRE_ERROR until kaia_cre_init has succeeded. */
int kaia_cre_doc_new(int width, int height, kaia_cre_doc **out);
void kaia_cre_doc_free(kaia_cre_doc *doc);

/* Loads a file. `loaded` is 1 when CREngine accepted it, 0 otherwise. */
int kaia_cre_doc_load(kaia_cre_doc *doc, const char *path, int *loaded);

/* The format CREngine detected for the loaded file: doc_format_t in its
 * bookformats.h (5 is EPUB, 1 is FB2, 3 is TXT, 6 is HTML). */
int kaia_cre_doc_format(kaia_cre_doc *doc, int32_t *out);

int kaia_cre_set_size(kaia_cre_doc *doc, int width, int height);
int kaia_cre_set_font_size(kaia_cre_doc *doc, int px);
/* The preferred font is process-wide in CREngine, so a caller that works with
 * several documents applies the face again before each operation. A repeat
 * call with the same face does not re-layout the document. */
int kaia_cre_set_font_face(kaia_cre_doc *doc, const char *face);
int kaia_cre_set_interline_percent(kaia_cre_doc *doc, int percent);
int kaia_cre_set_margins(kaia_cre_doc *doc, int left, int top, int right, int bottom);
int kaia_cre_set_embedded_styles(kaia_cre_doc *doc, int enabled);
int kaia_cre_set_embedded_fonts(kaia_cre_doc *doc, int enabled);
/* Base font weight in CSS units: 400 is regular and 700 is bold. CREngine
 * picks the nearest face and fakes the difference when the gap is 25 or more.
 * The engine clamps values to 1 to 999. */
int kaia_cre_set_font_weight(kaia_cre_doc *doc, int weight);
/* `dict` is a dictionary id such as "English_US.pattern", or NULL to turn hyphenation off. */
int kaia_cre_set_hyphenation(kaia_cre_doc *doc, const char *dict);
/* `css_file` is read when not NULL. `css_extra` is appended when not NULL. */
int kaia_cre_set_stylesheet(kaia_cre_doc *doc, const char *css_file, const char *css_extra);

/* Renders the current page at width x height. `out` holds width*height bytes
 * when `color` is 0 (gray8, 0 is black) and width*height*3 bytes when it is 1
 * (rgb24). Each side is at most 16384 pixels. */
int kaia_cre_render(kaia_cre_doc *doc, int width, int height, int color, uint8_t *out, size_t out_len);

int kaia_cre_page_count(kaia_cre_doc *doc, int32_t *out);
int kaia_cre_goto_page(kaia_cre_doc *doc, int32_t page);
int kaia_cre_current_page(kaia_cre_doc *doc, int32_t *out);
/* DOM text between page boundary anchors, with block separators as newlines. */
int kaia_cre_range_text(kaia_cre_doc *doc, const char *start, const char *end, char **out);
int kaia_cre_page_xpointer(kaia_cre_doc *doc, int32_t page, char **out);
/* Returns KAIA_CRE_NOT_FOUND when the xpointer is not in the document. */
int kaia_cre_xpointer_page(kaia_cre_doc *doc, const char *xpointer, int32_t *out);
/* Positive if a precedes b, negative if a follows b, zero if equal.
 * Returns KAIA_CRE_NOT_FOUND if either xpointer is invalid. */
int kaia_cre_compare_xpointers(kaia_cre_doc *doc, const char *a, const char *b, int32_t *out);
/* Returns KAIA_CRE_NOT_FOUND when no node is at the point. */
int kaia_cre_xpointer_at(kaia_cre_doc *doc, int32_t x, int32_t y, char **out);

/* Link boxes on the page on screen (call kaia_cre_goto_page first). */
int kaia_cre_page_links(kaia_cre_doc *doc, kaia_cre_link_box **out, size_t *count);
void kaia_cre_free_link_boxes(kaia_cre_link_box *boxes, size_t count);

/* Share of the page on screen that image elements cover, from 0 to 1. The view
 * must show one page in paged mode. Otherwise the call returns
 * KAIA_CRE_NOT_FOUND, and the caller treats the share as unknown. Call
 * kaia_cre_goto_page first. Images that overlap count twice before the cap. */
int kaia_cre_page_image_share(kaia_cre_doc *doc, float *out);

int kaia_cre_toc(kaia_cre_doc *doc, kaia_cre_toc_entry **out, size_t *count);
void kaia_cre_free_toc(kaia_cre_toc_entry *entries, size_t count);

/* Character boxes for the text between two xpointers, limited to the page
 * that is on screen (call kaia_cre_goto_page first). `xp1` may be NULL for
 * the end of the document. */
int kaia_cre_char_boxes(kaia_cre_doc *doc, const char *xp0, const char *xp1, kaia_cre_char_box **out, size_t *count);
void kaia_cre_free_boxes(kaia_cre_char_box *boxes);

/* The first visible character at or after `xp` on the page on screen (call
 * kaia_cre_goto_page first). The walk stops at that character, so the cost
 * does not grow with the document. Returns KAIA_CRE_NOT_FOUND when no visible
 * character follows `xp`. The box has line 0. */
int kaia_cre_first_char_box(kaia_cre_doc *doc, const char *xp, kaia_cre_char_box *out);

/* Searches the whole document. */
int kaia_cre_search(kaia_cre_doc *doc, const char *pattern, int case_insensitive, int max_hits, kaia_cre_range **out, size_t *count);
void kaia_cre_free_ranges(kaia_cre_range *ranges, size_t count);

/* Metadata strings. Each out pointer is set to NULL when the value is empty. */
int kaia_cre_metadata(kaia_cre_doc *doc, char **title, char **authors, char **language);

/* Cover image bytes in the format the file stores them. KAIA_CRE_NOT_FOUND when there is none. */
int kaia_cre_cover(kaia_cre_doc *doc, uint8_t **out, size_t *len);

void kaia_cre_free(void *ptr);

#ifdef __cplusplus
}
#endif

#endif /* KAIA_CRE_H */
