/*
 * C shim over KOReader's MuPDF build. See kaia_mupdf.h for the contract.
 *
 * Rules followed in this file:
 * - Every MuPDF call happens inside fz_try, or inside a helper that is only
 *   called from fz_try.
 * - No return, break or goto leaves a fz_try or fz_catch block. Results go
 *   through a status variable and are returned after the block.
 * - Variables assigned inside fz_try and read in fz_always or fz_catch are
 *   marked with fz_var.
 */

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <mupdf/fitz.h>

#include "kaia_mupdf.h"

/* Largest render side in device pixels. Guards malloc against absurd sizes. */
#define KAIA_MAX_SIDE 20000
/* Largest hit limit kaia_doc_search accepts. A larger request is clamped. */
#define KAIA_MAX_HITS 65536

struct kaia_doc {
    fz_context *ctx;
    fz_document *doc;
    char err[512];
};

static void set_err(kaia_doc *d, const char *msg)
{
    if (d)
        snprintf(d->err, sizeof d->err, "%s", msg ? msg : "unknown error");
}

static int arg_error(kaia_doc *d, const char *msg)
{
    set_err(d, msg);
    return KAIA_ERR_ARG;
}

static int need_doc(kaia_doc *d)
{
    if (!d)
        return KAIA_ERR_ARG;
    if (!d->doc)
        return arg_error(d, "no document open");
    return KAIA_OK;
}

/* Copies a string with malloc. Returns NULL when malloc fails. */
static char *dup_str(const char *s)
{
    size_t n = strlen(s) + 1;
    char *p = malloc(n);
    if (p)
        memcpy(p, s, n);
    return p;
}

/* Counts pages. Returns a status, the count is written to *count. */
static int page_count_of(kaia_doc *d, int *count)
{
    fz_context *ctx = d->ctx;
    int status = KAIA_OK;
    fz_try(ctx) {
        *count = fz_count_pages(ctx, d->doc);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        status = KAIA_ERR_MUPDF;
    }
    return status;
}

/* Checks that a document is open and that page is in range. */
static int check_page(kaia_doc *d, int page)
{
    int count = 0;
    int status = need_doc(d);
    if (status != KAIA_OK)
        return status;
    status = page_count_of(d, &count);
    if (status != KAIA_OK)
        return status;
    if (page < 0 || page >= count) {
        set_err(d, "page number out of range");
        return KAIA_ERR_RANGE;
    }
    return KAIA_OK;
}

static void quad_box(const fz_quad *q, float *x0, float *y0, float *x1, float *y1)
{
    float xs[4] = { q->ul.x, q->ur.x, q->ll.x, q->lr.x };
    float ys[4] = { q->ul.y, q->ur.y, q->ll.y, q->lr.y };
    int i;
    *x0 = *x1 = xs[0];
    *y0 = *y1 = ys[0];
    for (i = 1; i < 4; i++) {
        if (xs[i] < *x0) *x0 = xs[i];
        if (xs[i] > *x1) *x1 = xs[i];
        if (ys[i] < *y0) *y0 = ys[i];
        if (ys[i] > *y1) *y1 = ys[i];
    }
}

/* ------------------------------------------------------------------ */
/* Handle lifetime                                                     */
/* ------------------------------------------------------------------ */

int kaia_doc_new(kaia_doc **out)
{
    kaia_doc *d;
    fz_context *ctx;
    int status = KAIA_OK;

    if (!out)
        return KAIA_ERR_ARG;
    *out = NULL;

    d = calloc(1, sizeof *d);
    if (!d)
        return KAIA_ERR_NOMEM;

    ctx = fz_new_context(NULL, NULL, FZ_STORE_DEFAULT);
    if (!ctx) {
        free(d);
        return KAIA_ERR_NOMEM;
    }
    d->ctx = ctx;

    fz_try(ctx) {
        fz_register_document_handlers(ctx);
        fz_install_external_font_funcs(ctx);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        status = KAIA_ERR_MUPDF;
    }

    if (status != KAIA_OK) {
        fz_drop_context(ctx);
        free(d);
        return status;
    }
    *out = d;
    return KAIA_OK;
}

void kaia_doc_free(kaia_doc *d)
{
    if (!d)
        return;
    if (d->doc)
        fz_drop_document(d->ctx, d->doc);
    fz_drop_context(d->ctx);
    free(d);
}

const char *kaia_doc_error(const kaia_doc *d)
{
    return d ? d->err : "";
}

/* Opens a document. magic NULL means MuPDF chooses the handler from the path. */
static int open_impl(kaia_doc *d, const char *path, const char *magic)
{
    fz_context *ctx;
    fz_stream *stream = NULL;
    fz_document *doc = NULL;
    int count = 0;
    int status = KAIA_OK;

    if (!d)
        return KAIA_ERR_ARG;
    if (!path)
        return arg_error(d, "null path");
    ctx = d->ctx;

    fz_var(stream);
    fz_var(doc);
    fz_try(ctx) {
        if (magic) {
            stream = fz_open_file(ctx, path);
            doc = fz_open_document_with_stream(ctx, magic, stream);
        } else {
            doc = fz_open_document(ctx, path);
        }
        /* Counting pages surfaces files that open but cannot be parsed. */
        count = fz_count_pages(ctx, doc);
    }
    fz_always(ctx) {
        if (stream)
            fz_drop_stream(ctx, stream);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        if (doc)
            fz_drop_document(ctx, doc);
        status = KAIA_ERR_MUPDF;
    }
    if (status != KAIA_OK)
        return status;
    /* MuPDF repairs damaged files and can report zero pages for data it cannot
     * use. Treat that as a failed open. */
    if (count <= 0) {
        fz_drop_document(ctx, doc);
        set_err(d, "document has no pages");
        return KAIA_ERR_MUPDF;
    }

    if (d->doc)
        fz_drop_document(ctx, d->doc);
    d->doc = doc;
    return KAIA_OK;
}

int kaia_doc_open(kaia_doc *d, const char *path)
{
    return open_impl(d, path, NULL);
}

int kaia_doc_open_as(kaia_doc *d, const char *path, const char *magic)
{
    if (!magic)
        return arg_error(d, "null magic");
    return open_impl(d, path, magic);
}

/* ------------------------------------------------------------------ */
/* Document properties                                                 */
/* ------------------------------------------------------------------ */

int kaia_doc_page_count(kaia_doc *d, int *out)
{
    int status = need_doc(d);
    if (!out)
        return KAIA_ERR_ARG;
    if (status != KAIA_OK)
        return status;
    *out = 0;
    return page_count_of(d, out);
}

int kaia_doc_is_reflowable(kaia_doc *d, int *out)
{
    fz_context *ctx;
    int status = need_doc(d);
    int r = 0;
    if (!out)
        return KAIA_ERR_ARG;
    if (status != KAIA_OK)
        return status;
    ctx = d->ctx;
    fz_try(ctx) {
        r = fz_is_document_reflowable(ctx, d->doc);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        status = KAIA_ERR_MUPDF;
    }
    *out = (status == KAIA_OK) ? (r != 0) : 0;
    return status;
}

int kaia_doc_set_user_css(kaia_doc *d, const char *css)
{
    fz_context *ctx;
    int status = KAIA_OK;

    if (!d)
        return KAIA_ERR_ARG;
    ctx = d->ctx;
    /* MuPDF copies the string, so the caller's buffer only has to live for this call. */
    fz_try(ctx) {
        fz_set_user_css(ctx, css ? css : "");
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        status = KAIA_ERR_MUPDF;
    }
    return status;
}

int kaia_doc_layout(kaia_doc *d, float width, float height, float em)
{
    fz_context *ctx;
    int status = need_doc(d);
    if (status != KAIA_OK)
        return status;
    ctx = d->ctx;
    fz_try(ctx) {
        fz_layout_document(ctx, d->doc, width, height, em);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        status = KAIA_ERR_MUPDF;
    }
    return status;
}

int kaia_doc_metadata(kaia_doc *d, const char *key, char **out)
{
    fz_context *ctx;
    char small[512];
    char *big = NULL;
    char *res = NULL;
    int status;
    int n = -1;

    if (!out || !key)
        return KAIA_ERR_ARG;
    *out = NULL;
    status = need_doc(d);
    if (status != KAIA_OK)
        return status;
    ctx = d->ctx;

    fz_var(big);
    fz_var(res);
    fz_try(ctx) {
        n = fz_lookup_metadata(ctx, d->doc, key, small, sizeof small);
        if (n > 0 && (size_t)n <= sizeof small) {
            res = dup_str(small);
            if (!res)
                fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
        } else if (n > (int)sizeof small) {
            big = malloc((size_t)n);
            if (!big)
                fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
            n = fz_lookup_metadata(ctx, d->doc, key, big, (size_t)n);
            res = big;
            big = NULL;
        }
    }
    fz_always(ctx) {
        free(big);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        free(res);
        res = NULL;
        status = KAIA_ERR_MUPDF;
    }
    if (status == KAIA_OK)
        *out = res;
    return status;
}

/* ------------------------------------------------------------------ */
/* Geometry and rendering                                              */
/* ------------------------------------------------------------------ */

int kaia_doc_page_bounds(kaia_doc *d, int page_no, float box[4])
{
    fz_context *ctx;
    fz_page *page = NULL;
    int status;

    if (!box)
        return KAIA_ERR_ARG;
    status = check_page(d, page_no);
    if (status != KAIA_OK)
        return status;
    ctx = d->ctx;

    fz_var(page);
    fz_try(ctx) {
        fz_rect r;
        page = fz_load_page(ctx, d->doc, page_no);
        r = fz_bound_page(ctx, page);
        box[0] = r.x0;
        box[1] = r.y0;
        box[2] = r.x1;
        box[3] = r.y1;
    }
    fz_always(ctx) {
        if (page)
            fz_drop_page(ctx, page);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        status = KAIA_ERR_MUPDF;
    }
    return status;
}

int kaia_doc_render(kaia_doc *d, int page_no, float scale, const int *clip, int gray,
                    kaia_pixels *out)
{
    fz_context *ctx;
    fz_page *page = NULL;
    fz_pixmap *pix = NULL;
    fz_device *dev = NULL;
    unsigned char *buf = NULL;
    int status;

    if (!out)
        return KAIA_ERR_ARG;
    memset(out, 0, sizeof *out);
    status = check_page(d, page_no);
    if (status != KAIA_OK)
        return status;
    if (!(scale > 0.0f) || !isfinite(scale))
        return arg_error(d, "scale must be a positive number");
    ctx = d->ctx;

    fz_var(page);
    fz_var(pix);
    fz_var(dev);
    fz_var(buf);
    fz_try(ctx) {
        fz_matrix ctm;
        fz_rect bounds;
        fz_irect area;
        fz_colorspace *cs;
        long long area_w, area_h;
        int w, h, n, y, row;

        page = fz_load_page(ctx, d->doc, page_no);
        bounds = fz_bound_page(ctx, page);
        ctm = fz_scale(scale, scale);
        bounds = fz_transform_rect(bounds, ctm);
        if (clip)
            area = fz_make_irect(clip[0], clip[1], clip[2], clip[3]);
        else
            area = fz_round_rect(bounds);

        /* The corners can be anywhere in the int range, so subtract in 64 bits. */
        area_w = (long long)area.x1 - (long long)area.x0;
        area_h = (long long)area.y1 - (long long)area.y0;
        if (area_w <= 0 || area_h <= 0)
            fz_throw(ctx, FZ_ERROR_GENERIC, "render area is empty");
        if (area_w > KAIA_MAX_SIDE || area_h > KAIA_MAX_SIDE)
            fz_throw(ctx, FZ_ERROR_GENERIC, "render area is too large");
        w = (int)area_w;
        h = (int)area_h;

        cs = gray ? fz_device_gray(ctx) : fz_device_rgb(ctx);
        pix = fz_new_pixmap_with_bbox(ctx, cs, fz_make_irect(0, 0, w, h), NULL, 0);
        fz_clear_pixmap_with_value(ctx, pix, 0xff);

        /* Move the requested area to the pixmap origin. */
        ctm = fz_concat(ctm, fz_translate(-(float)area.x0, -(float)area.y0));
        dev = fz_new_draw_device(ctx, ctm, pix);
        fz_run_page(ctx, page, dev, fz_identity, NULL);
        fz_close_device(ctx, dev);

        n = pix->n;
        if (n != (gray ? 1 : 3))
            fz_throw(ctx, FZ_ERROR_GENERIC, "unexpected pixmap layout");
        row = w * n;
        buf = malloc((size_t)row * (size_t)h);
        if (!buf)
            fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
        for (y = 0; y < h; y++)
            memcpy(buf + (size_t)y * row, pix->samples + (ptrdiff_t)y * pix->stride, row);

        out->width = w;
        out->height = h;
        out->stride = row;
        out->format = gray ? 0 : 1;
        out->len = (size_t)row * (size_t)h;
        out->data = buf;
    }
    fz_always(ctx) {
        if (dev)
            fz_drop_device(ctx, dev);
        if (pix)
            fz_drop_pixmap(ctx, pix);
        if (page)
            fz_drop_page(ctx, page);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        free(buf);
        memset(out, 0, sizeof *out);
        status = KAIA_ERR_MUPDF;
    }
    return status;
}

/* ------------------------------------------------------------------ */
/* Outline                                                             */
/* ------------------------------------------------------------------ */

/* Resolves a link URI to a page. Returns -1 when it does not point inside
 * the document. Runs inside fz_try, so it may use nested try blocks. */
static int resolve_page(fz_context *ctx, fz_document *doc, const char *uri)
{
    int page = -1;
    fz_try(ctx) {
        float xp = 0.0f, yp = 0.0f;
        fz_location loc = fz_resolve_link(ctx, doc, uri, &xp, &yp);
        if (loc.page >= 0 || loc.chapter >= 0)
            page = fz_page_number_from_location(ctx, doc, loc);
    }
    fz_catch(ctx) {
        page = -1;
    }
    return page;
}

static int count_outline(const fz_outline *o)
{
    int n = 0;
    for (; o; o = o->next)
        n += 1 + count_outline(o->down);
    return n;
}

/* Fills items depth first. Each title is malloc'd. Runs inside fz_try. */
static void fill_outline(fz_context *ctx, fz_document *doc, const fz_outline *o, int level,
                         kaia_toc_item *items, int *idx)
{
    for (; o; o = o->next) {
        kaia_toc_item *it = &items[*idx];
        int page = -1;
        char *title;

        it->level = level;
        if (o->page.page >= 0 || o->page.chapter >= 0)
            page = fz_page_number_from_location(ctx, doc, o->page);
        /* Many EPUB outlines carry only a URI. Resolve it to a page. */
        if (page < 0 && o->uri)
            page = resolve_page(ctx, doc, o->uri);
        it->page = page;
        title = dup_str(o->title ? o->title : "");
        if (!title)
            fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
        it->title = title;
        (*idx)++;
        fill_outline(ctx, doc, o->down, level + 1, items, idx);
    }
}

void kaia_toc_free(kaia_toc_item *items, int count)
{
    int i;
    if (!items)
        return;
    for (i = 0; i < count; i++)
        free(items[i].title);
    free(items);
}

int kaia_doc_toc(kaia_doc *d, kaia_toc_item **items, int *count)
{
    fz_context *ctx;
    fz_outline *outline = NULL;
    kaia_toc_item *arr = NULL;
    int n = 0;
    int idx = 0;
    int status;

    if (!items || !count)
        return KAIA_ERR_ARG;
    *items = NULL;
    *count = 0;
    status = need_doc(d);
    if (status != KAIA_OK)
        return status;
    ctx = d->ctx;

    fz_var(outline);
    fz_var(arr);
    fz_var(n);
    fz_var(idx);
    fz_try(ctx) {
        outline = fz_load_outline(ctx, d->doc);
        n = count_outline(outline);
        if (n > 0) {
            arr = calloc((size_t)n, sizeof *arr);
            if (!arr)
                fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
            fill_outline(ctx, d->doc, outline, 1, arr, &idx);
        }
    }
    fz_always(ctx) {
        if (outline)
            fz_drop_outline(ctx, outline);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        kaia_toc_free(arr, idx);
        arr = NULL;
        status = KAIA_ERR_MUPDF;
    }
    if (status == KAIA_OK) {
        *items = arr;
        *count = n;
    }
    return status;
}

/* ------------------------------------------------------------------ */
/* Links                                                               */
/* ------------------------------------------------------------------ */

void kaia_links_free(kaia_link *links, int count)
{
    int i;
    if (!links)
        return;
    for (i = 0; i < count; i++)
        free(links[i].uri);
    free(links);
}

int kaia_doc_links(kaia_doc *d, int page_no, kaia_link **links, int *count)
{
    fz_context *ctx;
    fz_page *page = NULL;
    fz_link *list = NULL;
    kaia_link *arr = NULL;
    int n = 0;
    int idx = 0;
    int status;

    if (!links || !count)
        return KAIA_ERR_ARG;
    *links = NULL;
    *count = 0;
    status = check_page(d, page_no);
    if (status != KAIA_OK)
        return status;
    ctx = d->ctx;

    fz_var(page);
    fz_var(list);
    fz_var(arr);
    fz_var(n);
    fz_var(idx);
    fz_try(ctx) {
        fz_link *l;
        page = fz_load_page(ctx, d->doc, page_no);
        list = fz_load_links(ctx, page);
        for (l = list; l; l = l->next)
            n++;
        if (n > 0) {
            arr = calloc((size_t)n, sizeof *arr);
            if (!arr)
                fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
            for (l = list; l; l = l->next) {
                kaia_link *k = &arr[idx];
                k->x0 = l->rect.x0;
                k->y0 = l->rect.y0;
                k->x1 = l->rect.x1;
                k->y1 = l->rect.y1;
                k->uri = dup_str(l->uri ? l->uri : "");
                if (!k->uri)
                    fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
                k->page = resolve_page(ctx, d->doc, k->uri);
                idx++;
            }
        }
    }
    fz_always(ctx) {
        if (list)
            fz_drop_link(ctx, list);
        if (page)
            fz_drop_page(ctx, page);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        kaia_links_free(arr, idx);
        arr = NULL;
        status = KAIA_ERR_MUPDF;
    }
    if (status == KAIA_OK) {
        *links = arr;
        *count = n;
    }
    return status;
}

/* ------------------------------------------------------------------ */
/* Structured text and search                                          */
/* ------------------------------------------------------------------ */

void kaia_stext_free(kaia_stext *s)
{
    if (!s)
        return;
    free(s->blocks);
    free(s->lines);
    free(s->chars);
    memset(s, 0, sizeof *s);
}

static void stext_options(fz_stext_options *opts)
{
    memset(opts, 0, sizeof *opts);
    opts->scale = 1.0f;
}

int kaia_doc_stext(kaia_doc *d, int page_no, kaia_stext *out)
{
    fz_context *ctx;
    fz_page *page = NULL;
    fz_stext_page *text = NULL;
    int status;

    if (!out)
        return KAIA_ERR_ARG;
    memset(out, 0, sizeof *out);
    status = check_page(d, page_no);
    if (status != KAIA_OK)
        return status;
    ctx = d->ctx;

    fz_var(page);
    fz_var(text);
    fz_try(ctx) {
        fz_stext_options opts;
        const fz_stext_block *b;
        int nb = 0, nl = 0, nc = 0;
        int bi = 0, li = 0, ci = 0;

        stext_options(&opts);
        page = fz_load_page(ctx, d->doc, page_no);
        text = fz_new_stext_page_from_page(ctx, page, &opts);

        for (b = text->first_block; b; b = b->next) {
            nb++;
            if (b->type == FZ_STEXT_BLOCK_TEXT) {
                const fz_stext_line *l;
                for (l = b->u.t.first_line; l; l = l->next) {
                    const fz_stext_char *c;
                    nl++;
                    for (c = l->first_char; c; c = c->next)
                        nc++;
                }
            }
        }

        if (nb > 0) {
            out->blocks = calloc((size_t)nb, sizeof *out->blocks);
            if (!out->blocks)
                fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
        }
        if (nl > 0) {
            out->lines = calloc((size_t)nl, sizeof *out->lines);
            if (!out->lines)
                fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
        }
        if (nc > 0) {
            out->chars = calloc((size_t)nc, sizeof *out->chars);
            if (!out->chars)
                fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
        }
        out->block_count = nb;
        out->line_count = nl;
        out->char_count = nc;

        for (b = text->first_block; b; b = b->next) {
            kaia_stext_block *kb = &out->blocks[bi++];
            kb->x0 = b->bbox.x0;
            kb->y0 = b->bbox.y0;
            kb->x1 = b->bbox.x1;
            kb->y1 = b->bbox.y1;
            kb->first_line = li;
            kb->line_count = 0;
            if (b->type == FZ_STEXT_BLOCK_TEXT) {
                const fz_stext_line *l;
                kb->kind = 0;
                for (l = b->u.t.first_line; l; l = l->next) {
                    const fz_stext_char *c;
                    kaia_stext_line *kl = &out->lines[li++];
                    kb->line_count++;
                    kl->x0 = l->bbox.x0;
                    kl->y0 = l->bbox.y0;
                    kl->x1 = l->bbox.x1;
                    kl->y1 = l->bbox.y1;
                    kl->first_char = ci;
                    kl->char_count = 0;
                    for (c = l->first_char; c; c = c->next) {
                        kaia_stext_char *kc = &out->chars[ci++];
                        kl->char_count++;
                        kc->unicode = c->c;
                        quad_box(&c->quad, &kc->x0, &kc->y0, &kc->x1, &kc->y1);
                    }
                }
            } else if (b->type == FZ_STEXT_BLOCK_IMAGE) {
                kb->kind = 1;
            } else {
                kb->kind = 2;
            }
        }
    }
    fz_always(ctx) {
        if (text)
            fz_drop_stext_page(ctx, text);
        if (page)
            fz_drop_page(ctx, page);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        kaia_stext_free(out);
        status = KAIA_ERR_MUPDF;
    }
    return status;
}

int kaia_doc_search(kaia_doc *d, int page_no, const char *needle, int max_hits,
                    float **quads, int *count)
{
    fz_context *ctx;
    fz_page *page = NULL;
    fz_stext_page *text = NULL;
    fz_quad *hits = NULL;
    int *marks = NULL;
    float *flat = NULL;
    int n = 0;
    int status;

    if (!quads || !count)
        return KAIA_ERR_ARG;
    *quads = NULL;
    *count = 0;
    if (!needle || !*needle || max_hits <= 0)
        return arg_error(d, "empty search text or bad hit limit");
    if (max_hits > KAIA_MAX_HITS)
        max_hits = KAIA_MAX_HITS;
    status = check_page(d, page_no);
    if (status != KAIA_OK)
        return status;
    ctx = d->ctx;

    fz_var(page);
    fz_var(text);
    fz_var(hits);
    fz_var(marks);
    fz_var(flat);
    fz_var(n);
    fz_try(ctx) {
        fz_stext_options opts;
        int i;

        stext_options(&opts);
        page = fz_load_page(ctx, d->doc, page_no);
        text = fz_new_stext_page_from_page(ctx, page, &opts);
        /* max_hits is at most KAIA_MAX_HITS, so these products fit in 32-bit
         * size_t. The checks keep that true if the constant grows. */
        if ((size_t)max_hits > SIZE_MAX / sizeof *hits || (size_t)max_hits > SIZE_MAX / sizeof *marks)
            fz_throw(ctx, FZ_ERROR_GENERIC, "hit limit is too large");
        hits = malloc((size_t)max_hits * sizeof *hits);
        marks = malloc((size_t)max_hits * sizeof *marks);
        if (!hits || !marks)
            fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");

        n = fz_search_stext_page(ctx, text, needle, marks, hits, max_hits);
        if (n < 0)
            n = 0;
        flat = malloc((size_t)(n > 0 ? n : 1) * 8 * sizeof *flat);
        if (!flat)
            fz_throw(ctx, FZ_ERROR_GENERIC, "out of memory");
        for (i = 0; i < n; i++) {
            const fz_quad *q = &hits[i];
            float *f = &flat[i * 8];
            f[0] = q->ul.x; f[1] = q->ul.y;
            f[2] = q->ur.x; f[3] = q->ur.y;
            f[4] = q->ll.x; f[5] = q->ll.y;
            f[6] = q->lr.x; f[7] = q->lr.y;
        }
    }
    fz_always(ctx) {
        free(hits);
        free(marks);
        if (text)
            fz_drop_stext_page(ctx, text);
        if (page)
            fz_drop_page(ctx, page);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        free(flat);
        flat = NULL;
        n = 0;
        status = KAIA_ERR_MUPDF;
    }
    if (status == KAIA_OK) {
        *quads = flat;
        *count = n;
    }
    return status;
}

/* ------------------------------------------------------------------ */
/* Locations                                                           */
/* ------------------------------------------------------------------ */

int kaia_doc_resolve_uri(kaia_doc *d, const char *uri, int *page)
{
    fz_context *ctx;
    int status;
    int p = -1;

    if (!page || !uri)
        return KAIA_ERR_ARG;
    *page = -1;
    status = need_doc(d);
    if (status != KAIA_OK)
        return status;
    ctx = d->ctx;
    fz_try(ctx) {
        p = resolve_page(ctx, d->doc, uri);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        status = KAIA_ERR_MUPDF;
    }
    if (status == KAIA_OK)
        *page = p;
    return status;
}

int kaia_doc_location_page(kaia_doc *d, int chapter, int page, int *out)
{
    fz_context *ctx;
    int status;
    int p = -1;

    if (!out)
        return KAIA_ERR_ARG;
    *out = -1;
    status = need_doc(d);
    if (status != KAIA_OK)
        return status;
    ctx = d->ctx;
    fz_try(ctx) {
        p = fz_page_number_from_location(ctx, d->doc, fz_make_location(chapter, page));
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        status = KAIA_ERR_MUPDF;
    }
    if (status == KAIA_OK)
        *out = p;
    return status;
}

void kaia_free(void *p)
{
    free(p);
}

/* Writes trusted, locally generated HTML. All handles belong to d's context. */
int kaia_write_story_pdf(kaia_doc *d, const char *path, const char *html,
                         const char *css, const char *fonts)
{
    fz_context *ctx;
    fz_buffer *buffer = NULL;
    fz_archive *archive = NULL;
    fz_story *story = NULL;
    fz_story *footer = NULL;
    fz_buffer *footer_buffer = NULL;
    fz_document_writer *writer = NULL;
    int status = KAIA_OK;
    const fz_rect page = { 0, 0, 420, 595 };
    const fz_rect body = { 36, 36, 384, 547 };
    const fz_rect foot = { 36, 560, 384, 585 };
    if (!d || !path || !html || !css || !fonts)
        return KAIA_ERR_ARG;
    ctx = d->ctx;
    fz_var(buffer);
    fz_var(archive);
    fz_var(story);
    fz_var(footer);
    fz_var(footer_buffer);
    fz_var(writer);
    fz_try(ctx) {
        archive = fz_open_directory(ctx, fonts);
        buffer = fz_new_buffer_from_copied_data(ctx, (const unsigned char *)html, strlen(html));
        story = fz_new_story(ctx, buffer, css, 11, archive);
        writer = fz_new_pdf_writer(ctx, path, "compress=yes");
        int more = 1;
        int number = 0;
        while (more) {
            fz_rect filled;
            char label[96];
            /* Bound malformed layouts that never consume their next block. */
            if (++number > 10000)
                fz_throw(ctx, FZ_ERROR_GENERIC, "notes export exceeds 10000 pages");
            more = fz_place_story_flags(ctx, story, body, &filled, FZ_PLACE_STORY_FLAG_NO_OVERFLOW);
            if (more == FZ_PLACE_STORY_RETURN_OVERFLOW_WIDTH)
                fz_throw(ctx, FZ_ERROR_GENERIC, "notes export contains text too wide for the page");
            fz_device *dev = fz_begin_page(ctx, writer, page);
            fz_draw_story(ctx, story, dev, fz_identity);
            snprintf(label, sizeof label, "<p style='text-align:center;font-size:9pt'>%d</p>", number);
            footer_buffer = fz_new_buffer_from_copied_data(ctx, (const unsigned char *)label, strlen(label));
            footer = fz_new_story(ctx, footer_buffer, css, 9, archive);
            fz_place_story(ctx, footer, foot, &filled);
            fz_draw_story(ctx, footer, dev, fz_identity);
            fz_drop_story(ctx, footer);
            footer = NULL;
            fz_drop_buffer(ctx, footer_buffer);
            footer_buffer = NULL;
            fz_end_page(ctx, writer);
        }
        fz_close_document_writer(ctx, writer);
    }
    fz_always(ctx) {
        fz_drop_document_writer(ctx, writer);
        fz_drop_story(ctx, footer);
        fz_drop_buffer(ctx, footer_buffer);
        fz_drop_story(ctx, story);
        fz_drop_buffer(ctx, buffer);
        fz_drop_archive(ctx, archive);
    }
    fz_catch(ctx) {
        set_err(d, fz_caught_message(ctx));
        status = KAIA_ERR_MUPDF;
    }
    return status;
}
