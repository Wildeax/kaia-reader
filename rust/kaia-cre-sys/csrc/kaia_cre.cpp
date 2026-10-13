// C ABI over KOReader's CREngine build. The call sequences follow
// base/cre.cpp (KOReader's Lua binding) and frontend/document/credocument.lua.
// Every entry point runs under one process-wide lock and catches C++
// exceptions, so nothing unwinds into Rust. CREngine keeps global state
// (font manager, hyphenation, document instances), so the lock is required.

#include "kaia_cre.h"

#include <cstdlib>
#include <cstring>
#include <mutex>
#include <stdexcept>
#include <string>
#include <vector>

#include "lvdocview.h"
#include "lvdrawbuf.h"
#include "lvfntman.h"
#include "lvstream.h"
#include "lvstring.h"
#include "lvtinydom.h"
#include "hyphman.h"
#include "props.h"

extern LVFontManager *fontMan;

struct kaia_cre_doc {
    LVDocView *view;
};

namespace {

std::mutex g_lock;
bool g_inited = false;
thread_local std::string g_error;

// Thrown by the shim for errors it detects. The guard turns it into a return code.
struct CreError : std::runtime_error {
    using std::runtime_error::runtime_error;
};

[[noreturn]] void fail(const std::string &msg) { throw CreError(msg); }

// Runs `body` under the lock and converts every failure into a return code.
template <class F>
int guard(F body) {
    try {
        std::lock_guard<std::mutex> hold(g_lock);
        return body();
    } catch (const CreError &e) {
        g_error = e.what();
    } catch (const std::exception &e) {
        g_error = std::string("CREngine: ") + e.what();
    } catch (...) {
        g_error = "CREngine: unknown exception";
    }
    return KAIA_CRE_ERROR;
}

char *dup_utf8(const lString32 &s) {
    lString8 u8 = UnicodeToUtf8(s);
    char *out = static_cast<char *>(std::malloc(u8.length() + 1));
    if (!out) {
        fail("out of memory");
    }
    std::memcpy(out, u8.c_str(), u8.length());
    out[u8.length()] = '\0';
    return out;
}

char *dup_utf8(const std::string &s) {
    char *out = static_cast<char *>(std::malloc(s.size() + 1));
    if (!out) {
        fail("out of memory");
    }
    std::memcpy(out, s.data(), s.size());
    out[s.size()] = '\0';
    return out;
}

LVDocView *view_of(kaia_cre_doc *doc) {
    if (!doc || !doc->view) {
        fail("document handle is closed");
    }
    return doc->view;
}

ldomDocument *dom_of(kaia_cre_doc *doc) {
    ldomDocument *dom = view_of(doc)->getDocument();
    if (!dom) {
        fail("no document is loaded");
    }
    return dom;
}

// Parses an xpointer. Returns a null pointer when it is not in the document.
ldomXPointer parse_xpointer(kaia_cre_doc *doc, const char *text) {
    return dom_of(doc)->createXPointer(lString32(text));
}

// Same rule as docToWindowRect in base/cre.cpp: maps a document rect to the
// visible page. False when the rect is not on the page.
bool doc_to_window_rect(LVDocView *tv, lvRect &rc) {
    lvPoint topLeft = rc.topLeft();
    lvPoint bottomRight = rc.bottomRight();
    bool topInPage = false;
    bool bottomInPage = false;
    if (tv->docToWindowPoint(topLeft)) {
        rc.setTopLeft(topLeft);
        topInPage = true;
    }
    if (tv->docToWindowPoint(bottomRight, true)) {
        rc.setBottomRight(bottomRight);
        bottomInPage = true;
    }
    if (topInPage && bottomInPage) {
        return true;
    }
    if (bottomInPage && !topInPage) {
        topLeft = rc.topLeft();
        if (tv->docToWindowPoint(topLeft, false, true)) {
            if (bottomRight.y - topLeft.y <= 0) {
                if (!tv->docToWindowPoint(bottomRight)) {
                    return false;
                }
            }
            rc.setTopLeft(topLeft);
            return true;
        }
    } else if (topInPage && !bottomInPage) {
        bottomRight = rc.bottomRight();
        if (tv->docToWindowPoint(bottomRight, true, true)) {
            rc.setBottomRight(bottomRight);
            return true;
        }
    }
    return false;
}

void walk_toc(LVTocItem *item, std::vector<kaia_cre_toc_entry> &out) {
    int n = item->getChildCount();
    for (int i = 0; i < n; i++) {
        LVTocItem *child = item->getChild(i);
        kaia_cre_toc_entry e{};
        e.level = child->getLevel();
        e.page = child->getPage();
        e.title = dup_utf8(child->getName());
        e.xpointer = dup_utf8(child->getPath());
        out.push_back(e);
        walk_toc(child, out);
    }
}

void free_strings(char *a, char *b) {
    std::free(a);
    std::free(b);
}

} // namespace

extern "C" {

const char *kaia_cre_last_error(void) { return g_error.c_str(); }

int kaia_cre_init(const char *const *font_files, size_t font_count, const char *hyph_dir, int *registered) {
    return guard([&]() -> int {
        int accepted = 0;
        if (!g_inited) {
            if (!InitFontManager(lString8())) {
                fail("cannot start the CREngine font manager");
            }
            // KOReader's default (default_cre_storage_size_factor in
            // credocument.lua). It raises the in-memory limits of the DOM
            // storage, so large books do not hit them.
            setStorageMaxUncompressedSizeFactor(40.0f);
            for (size_t i = 0; i < font_count; i++) {
                if (font_files[i] && fontMan->RegisterFont(lString8(font_files[i]))) {
                    accepted++;
                }
            }
            if (hyph_dir) {
                std::string dir = hyph_dir;
                if (!dir.empty() && dir.back() != '/') {
                    dir += '/';
                }
                HyphMan::initDictionaries(lString32(dir.c_str()));
            }
            g_inited = true;
        }
        if (registered) {
            *registered = accepted;
        }
        return KAIA_CRE_OK;
    });
}

int kaia_cre_doc_new(int width, int height, kaia_cre_doc **out) {
    return guard([&]() -> int {
        if (width <= 0 || height <= 0 || !out) {
            fail("view size must be positive");
        }
        // Same constructor and mode as newDocView in base/cre.cpp. The page
        // header is off so the page holds only the text.
        LVDocView *view = new LVDocView(-1, true);
        view->setViewMode(DVM_PAGES, -1);
        view->Resize(width, height);
        view->setPageHeaderInfo(0);
        // The fallback values from readDefaults in base/cre.cpp, used when no
        // cr3.ini exists: autohinting (2) and harfbuzz kerning (3).
        CRPropRef props = LVCreatePropsContainer();
        props->setInt("font.hinting.mode", 2);
        props->setInt("font.kerning.mode", 3);
        view->propsApply(props);
        kaia_cre_doc *doc = new kaia_cre_doc{view};
        *out = doc;
        return KAIA_CRE_OK;
    });
}

void kaia_cre_doc_free(kaia_cre_doc *doc) {
    if (!doc) {
        return;
    }
    try {
        std::lock_guard<std::mutex> hold(g_lock);
        if (doc->view) {
            doc->view->close();
            delete doc->view;
        }
    } catch (...) {
        // Freeing must not fail. The handle is gone either way.
    }
    delete doc;
}

int kaia_cre_doc_load(kaia_cre_doc *doc, const char *path, int *loaded) {
    return guard([&]() -> int {
        LVDocView *view = view_of(doc);
        bool ok = view->LoadDocument(path, false);
        if (loaded) {
            *loaded = (ok && view->getDocument()) ? 1 : 0;
        }
        return KAIA_CRE_OK;
    });
}

int kaia_cre_doc_format(kaia_cre_doc *doc, int32_t *out) {
    return guard([&]() -> int {
        if (!out) {
            fail("null output");
        }
        *out = static_cast<int32_t>(view_of(doc)->getDocFormat());
        return KAIA_CRE_OK;
    });
}

int kaia_cre_set_size(kaia_cre_doc *doc, int width, int height) {
    return guard([&]() -> int {
        if (width <= 0 || height <= 0) {
            fail("view size must be positive");
        }
        view_of(doc)->Resize(width, height);
        return KAIA_CRE_OK;
    });
}

int kaia_cre_set_font_size(kaia_cre_doc *doc, int px) {
    return guard([&]() -> int {
        view_of(doc)->setFontSize(px);
        return KAIA_CRE_OK;
    });
}

int kaia_cre_set_font_face(kaia_cre_doc *doc, const char *face) {
    return guard([&]() -> int {
        LVDocView *view = view_of(doc);
        view->setDefaultFontFace(lString8(face));
        // Same bias as CreDocument:setFontFace in credocument.lua, so the
        // requested face wins over real italic or bold matches.
        fontMan->SetAsPreferredFontWithBias(lString8(face), 1 + 128 * 5 + 256 * 5, true);
        return KAIA_CRE_OK;
    });
}

int kaia_cre_set_interline_percent(kaia_cre_doc *doc, int percent) {
    return guard([&]() -> int {
        view_of(doc)->setDefaultInterlineSpace(percent);
        return KAIA_CRE_OK;
    });
}

int kaia_cre_set_margins(kaia_cre_doc *doc, int left, int top, int right, int bottom) {
    return guard([&]() -> int {
        LVDocView *view = view_of(doc);
        lvRect rc;
        rc.left = left;
        rc.top = top;
        rc.right = right;
        rc.bottom = bottom;
        view->setPageMargins(rc);
        return KAIA_CRE_OK;
    });
}

static int set_int_property(kaia_cre_doc *doc, const char *name, int value) {
    CRPropRef props = LVCreatePropsContainer();
    props->setInt(name, value);
    view_of(doc)->propsApply(props);
    return KAIA_CRE_OK;
}

int kaia_cre_set_embedded_styles(kaia_cre_doc *doc, int enabled) {
    return guard([&]() -> int { return set_int_property(doc, "crengine.doc.embedded.styles.enabled", enabled ? 1 : 0); });
}

int kaia_cre_set_embedded_fonts(kaia_cre_doc *doc, int enabled) {
    return guard([&]() -> int { return set_int_property(doc, "crengine.doc.embedded.fonts.enabled", enabled ? 1 : 0); });
}

int kaia_cre_set_hyphenation(kaia_cre_doc *doc, const char *dict) {
    return guard([&]() -> int {
        LVDocView *view = view_of(doc);
        CRPropRef props = LVCreatePropsContainer();
        // KOReader's setHyphDictionary writes this property. "@none" is
        // PROP_HYPHENATION_DICT_VALUE_NONE in CREngine's lvdocviewprops.h.
        props->setString("crengine.hyphenation.directory", dict ? dict : "@none");
        view->propsApply(props);
        return KAIA_CRE_OK;
    });
}

int kaia_cre_set_stylesheet(kaia_cre_doc *doc, const char *css_file, const char *css_extra) {
    return guard([&]() -> int {
        LVDocView *view = view_of(doc);
        lString8 css;
        if (css_file && !LVLoadStylesheetFile(lString32(css_file), css)) {
            css = lString8(); // Unreadable file: continue with the extra CSS only.
        }
        if (css_extra) {
            css << "\r\n" << lString8(css_extra);
        }
        // false: skip substituteCssMacros(), as cre.cpp does.
        view->setStyleSheet(css, false);
        return KAIA_CRE_OK;
    });
}

int kaia_cre_render(kaia_cre_doc *doc, int width, int height, int color, uint8_t *out, size_t out_len) {
    return guard([&]() -> int {
        LVDocView *view = view_of(doc);
        if (width <= 0 || height <= 0) {
            fail("render size must be positive");
        }
        size_t pixels = static_cast<size_t>(width) * static_cast<size_t>(height);
        size_t need = color ? pixels * 3 : pixels;
        if (!out || out_len < need) {
            fail("render buffer is too small");
        }
        view->Resize(width, height);
        view->Render();
        if (!color) {
            LVGrayDrawBuf buf(width, height, 8, out);
            buf.setInvertImages(false);
            view->Draw(buf, false);
            return KAIA_CRE_OK;
        }
        // CREngine draws BGRA with an inverted alpha byte. Keep B, G, R in
        // RGB order and drop alpha.
        std::vector<uint8_t> tmp(pixels * 4, 0xFF);
        LVColorDrawBuf buf(width, height, tmp.data(), 32);
        buf.setInvertImages(false);
        view->Draw(buf, false);
        for (size_t i = 0; i < pixels; i++) {
            out[i * 3 + 0] = tmp[i * 4 + 2];
            out[i * 3 + 1] = tmp[i * 4 + 1];
            out[i * 3 + 2] = tmp[i * 4 + 0];
        }
        return KAIA_CRE_OK;
    });
}

int kaia_cre_page_count(kaia_cre_doc *doc, int32_t *out) {
    return guard([&]() -> int {
        if (!out) {
            fail("null output");
        }
        LVDocView *view = view_of(doc);
        view->Render();
        *out = view->getPageCount(false);
        return KAIA_CRE_OK;
    });
}

int kaia_cre_goto_page(kaia_cre_doc *doc, int32_t page) {
    return guard([&]() -> int {
        LVDocView *view = view_of(doc);
        view->Render();
        view->goToPage(page, false, true, false);
        return KAIA_CRE_OK;
    });
}

int kaia_cre_current_page(kaia_cre_doc *doc, int32_t *out) {
    return guard([&]() -> int {
        if (!out) {
            fail("null output");
        }
        LVDocView *view = view_of(doc);
        view->Render();
        *out = view->getCurPage(false);
        return KAIA_CRE_OK;
    });
}

int kaia_cre_page_xpointer(kaia_cre_doc *doc, int32_t page, char **out) {
    return guard([&]() -> int {
        if (!out) {
            fail("null output");
        }
        LVDocView *view = view_of(doc);
        view->Render();
        ldomXPointer xp = view->getPageBookmark(page, true, false);
        if (xp.isNull()) {
            return KAIA_CRE_NOT_FOUND;
        }
        *out = dup_utf8(xp.toString());
        return KAIA_CRE_OK;
    });
}

int kaia_cre_xpointer_page(kaia_cre_doc *doc, const char *xpointer, int32_t *out) {
    return guard([&]() -> int {
        if (!out) {
            fail("null output");
        }
        LVDocView *view = view_of(doc);
        view->Render();
        ldomXPointer xp = parse_xpointer(doc, xpointer);
        if (xp.isNull()) {
            return KAIA_CRE_NOT_FOUND;
        }
        *out = view->getBookmarkPage(xp, false);
        return KAIA_CRE_OK;
    });
}

int kaia_cre_xpointer_at(kaia_cre_doc *doc, int32_t x, int32_t y, char **out) {
    return guard([&]() -> int {
        if (!out) {
            fail("null output");
        }
        LVDocView *view = view_of(doc);
        view->Render();
        ldomXPointer xp = view->getNodeByPoint(lvPoint(x, y));
        if (xp.isNull()) {
            return KAIA_CRE_NOT_FOUND;
        }
        *out = dup_utf8(xp.toString());
        return KAIA_CRE_OK;
    });
}

int kaia_cre_page_links(kaia_cre_doc *doc, kaia_cre_link_box **out, size_t *count) {
    return guard([&]() -> int {
        if (!out || !count) {
            fail("null output");
        }
        *out = nullptr;
        *count = 0;
        LVDocView *view = view_of(doc);
        view->Render();
        ldomDocument *dom = dom_of(doc);
        // Same walk as getPageLinks in base/cre.cpp: links on the current page,
        // one box per segment rect.
        ldomXRangeList links;
        view->getCurrentPageLinks(links);
        std::vector<kaia_cre_link_box> boxes;
        for (int i = 0; i < links.length(); i++) {
            ldomXPointer a_xpointer;
            lString32 href = links[i]->getHRef(a_xpointer);
            int32_t target = -1;
            if (!href.empty() && href[0] == '#') {
                ldomNode *node = dom->getElementById(href.c_str() + 1);
                if (node) {
                    target = view->getBookmarkPage(ldomXPointer(node, 0), false);
                }
            }
            ldomXRange range(*links[i]);
            LVArray<lvRect> rects;
            range.getSegmentRects(rects, true);
            for (int k = 0; k < rects.length(); k++) {
                lvRect rc = rects[k];
                if (rc.isEmpty() || !doc_to_window_rect(view, rc)) {
                    continue;
                }
                kaia_cre_link_box b{};
                b.x0 = static_cast<float>(rc.left);
                b.y0 = static_cast<float>(rc.top);
                b.x1 = static_cast<float>(rc.right);
                b.y1 = static_cast<float>(rc.bottom);
                b.page = target;
                b.href = dup_utf8(href);
                boxes.push_back(b);
            }
        }
        *count = boxes.size();
        if (boxes.empty()) {
            return KAIA_CRE_OK;
        }
        *out = static_cast<kaia_cre_link_box *>(std::malloc(sizeof(kaia_cre_link_box) * boxes.size()));
        if (!*out) {
            fail("out of memory");
        }
        std::memcpy(*out, boxes.data(), sizeof(kaia_cre_link_box) * boxes.size());
        return KAIA_CRE_OK;
    });
}

void kaia_cre_free_link_boxes(kaia_cre_link_box *boxes, size_t count) {
    if (!boxes) {
        return;
    }
    for (size_t i = 0; i < count; i++) {
        std::free(boxes[i].href);
    }
    std::free(boxes);
}

int kaia_cre_toc(kaia_cre_doc *doc, kaia_cre_toc_entry **out, size_t *count) {
    return guard([&]() -> int {
        if (!out || !count) {
            fail("null output");
        }
        std::vector<kaia_cre_toc_entry> entries;
        try {
            walk_toc(view_of(doc)->getToc(), entries);
        } catch (...) {
            for (auto &e : entries) {
                std::free(e.title);
                std::free(e.xpointer);
            }
            throw;
        }
        *count = entries.size();
        if (entries.empty()) {
            *out = nullptr;
            return KAIA_CRE_OK;
        }
        *out = static_cast<kaia_cre_toc_entry *>(std::malloc(sizeof(kaia_cre_toc_entry) * entries.size()));
        if (!*out) {
            fail("out of memory");
        }
        std::memcpy(*out, entries.data(), sizeof(kaia_cre_toc_entry) * entries.size());
        return KAIA_CRE_OK;
    });
}

void kaia_cre_free_toc(kaia_cre_toc_entry *entries, size_t count) {
    if (!entries) {
        return;
    }
    for (size_t i = 0; i < count; i++) {
        free_strings(entries[i].title, entries[i].xpointer);
    }
    std::free(entries);
}

int kaia_cre_char_boxes(kaia_cre_doc *doc, const char *xp0, const char *xp1, kaia_cre_char_box **out, size_t *count) {
    return guard([&]() -> int {
        if (!out || !count || !xp0) {
            fail("null argument");
        }
        LVDocView *view = view_of(doc);
        view->Render();
        ldomDocument *dom = dom_of(doc);
        ldomXPointer start = dom->createXPointer(lString32(xp0));
        ldomXPointer end = xp1 ? dom->createXPointer(lString32(xp1))
                               : ldomXPointer(dom->getRootNode(), dom->getRootNode()->getChildCount());
        *out = nullptr;
        *count = 0;
        if (start.isNull() || end.isNull()) {
            return KAIA_CRE_NOT_FOUND;
        }
        ldomXRange range(start, end);
        if (range.getStart().isNull() || range.getEnd().isNull()) {
            return KAIA_CRE_NOT_FOUND;
        }
        range.sort();

        // Walk every visible character in the range, punctuation and spaces
        // included. ldomWord would drop punctuation, because CREngine words
        // hold letters only.
        std::vector<kaia_cre_char_box> boxes;
        int line = 0;
        lvRect prev;
        bool have_prev = false;
        ldomNode *text_node = nullptr;
        lString32 text;
        ldomXPointerEx cur(range.getStart());
        ldomXPointerEx last(range.getEnd());
        const long kMaxSteps = 2000000;
        for (long steps = 0; cur.compare(last) <= 0 && steps < kMaxSteps; steps++) {
            if (cur.isText()) {
                ldomNode *node = cur.getNode();
                if (node != text_node) {
                    text_node = node;
                    text = node->getText();
                }
                int off = cur.getOffset();
                lvRect rc;
                if (off >= 0 && off < text.length() && ldomXPointer(node, off).getRectEx(rc) &&
                    doc_to_window_rect(view, rc)) {
                    if (have_prev && rc.top > prev.top + (prev.height() / 2)) {
                        line++;
                    }
                    kaia_cre_char_box b{};
                    b.x0 = static_cast<float>(rc.left);
                    b.y0 = static_cast<float>(rc.top);
                    b.x1 = static_cast<float>(rc.right);
                    b.y1 = static_cast<float>(rc.bottom);
                    b.line = line;
                    b.codepoint = static_cast<uint32_t>(text[off]);
                    boxes.push_back(b);
                    prev = rc;
                    have_prev = true;
                }
            }
            if (!cur.nextVisibleChar()) {
                break;
            }
        }
        *count = boxes.size();
        if (boxes.empty()) {
            return KAIA_CRE_OK;
        }
        *out = static_cast<kaia_cre_char_box *>(std::malloc(sizeof(kaia_cre_char_box) * boxes.size()));
        if (!*out) {
            fail("out of memory");
        }
        std::memcpy(*out, boxes.data(), sizeof(kaia_cre_char_box) * boxes.size());
        return KAIA_CRE_OK;
    });
}

void kaia_cre_free_boxes(kaia_cre_char_box *boxes) { std::free(boxes); }

int kaia_cre_search(kaia_cre_doc *doc, const char *pattern, int case_insensitive, int max_hits, kaia_cre_range **out,
                    size_t *count) {
    return guard([&]() -> int {
        if (!out || !count || !pattern) {
            fail("null argument");
        }
        *out = nullptr;
        *count = 0;
        LVDocView *view = view_of(doc);
        view->Render();
        ldomDocument *dom = dom_of(doc);
        lString32 needle(pattern);
        if (needle.empty() || max_hits <= 0) {
            return KAIA_CRE_OK;
        }
        int full = view->GetFullHeight();
        ldomXRangeList hits;
        // Search the whole document in one pass: y from 0 to the full height.
        dom->findText(needle, case_insensitive != 0, false, 0, full, hits, max_hits, full);
        std::vector<kaia_cre_range> ranges;
        for (int i = 0; i < hits.length(); i++) {
            kaia_cre_range r{};
            r.start = dup_utf8(hits[i]->getStart().toString());
            r.end = dup_utf8(hits[i]->getEnd().toString());
            ranges.push_back(r);
        }
        *count = ranges.size();
        if (ranges.empty()) {
            return KAIA_CRE_OK;
        }
        *out = static_cast<kaia_cre_range *>(std::malloc(sizeof(kaia_cre_range) * ranges.size()));
        if (!*out) {
            fail("out of memory");
        }
        std::memcpy(*out, ranges.data(), sizeof(kaia_cre_range) * ranges.size());
        return KAIA_CRE_OK;
    });
}

void kaia_cre_free_ranges(kaia_cre_range *ranges, size_t count) {
    if (!ranges) {
        return;
    }
    for (size_t i = 0; i < count; i++) {
        free_strings(ranges[i].start, ranges[i].end);
    }
    std::free(ranges);
}

int kaia_cre_metadata(kaia_cre_doc *doc, char **title, char **authors, char **language) {
    return guard([&]() -> int {
        LVDocView *view = view_of(doc);
        if (title) {
            *title = nullptr;
            lString32 v = view->getTitle();
            if (!v.empty()) {
                *title = dup_utf8(v);
            }
        }
        if (authors) {
            *authors = nullptr;
            lString32 v = view->getAuthors();
            if (!v.empty()) {
                *authors = dup_utf8(v);
            }
        }
        if (language) {
            *language = nullptr;
            lString32 v = view->getLanguage();
            if (!v.empty()) {
                *language = dup_utf8(v);
            }
        }
        return KAIA_CRE_OK;
    });
}

int kaia_cre_cover(kaia_cre_doc *doc, uint8_t **out, size_t *len) {
    return guard([&]() -> int {
        if (!out || !len) {
            fail("null output");
        }
        *out = nullptr;
        *len = 0;
        LVStreamRef stream = view_of(doc)->getCoverPageImageStream();
        if (stream.isNull()) {
            return KAIA_CRE_NOT_FOUND;
        }
        lvsize_t size = stream->GetSize();
        if (size == 0) {
            return KAIA_CRE_NOT_FOUND;
        }
        uint8_t *buf = static_cast<uint8_t *>(std::malloc(size));
        if (!buf) {
            fail("out of memory");
        }
        lvsize_t got = 0;
        stream->SetPos(0);
        if (stream->Read(buf, size, &got) != LVERR_OK || got != size) {
            std::free(buf);
            fail("cannot read the cover image");
        }
        *out = buf;
        *len = size;
        return KAIA_CRE_OK;
    });
}

void kaia_cre_free(void *ptr) { std::free(ptr); }

} // extern "C"
