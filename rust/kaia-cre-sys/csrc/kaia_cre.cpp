// C ABI over KOReader's CREngine build. The call sequences follow
// base/cre.cpp (KOReader's Lua binding) and frontend/document/credocument.lua.
// Every entry point runs under one process-wide lock and catches C++
// exceptions, so nothing unwinds into Rust. CREngine keeps global state
// (font manager, hyphenation, document instances), so the lock is required.

#include "kaia_cre.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <mutex>
#include <set>
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
#include "footnotes.h"

extern LVFontManager *fontMan;

struct kaia_cre_doc {
    LVDocView *view;
};

namespace {

std::mutex g_lock;
bool g_inited = false;

// Largest side of a rendered page in pixels. Also bounds the size checks below.
constexpr int kMaxRenderSide = 16384;

// Multiplies two sizes. False when the product does not fit in size_t.
bool mul_size(size_t a, size_t b, size_t *out) { return !__builtin_mul_overflow(a, b, out); }
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

// CREngine's font manager is a global that kaia_cre_init creates. Without it,
// code that reaches for `fontMan` dereferences null.
void require_init() {
    if (!g_inited || !fontMan) {
        fail("CREngine is not initialized: call kaia_cre_init first");
    }
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
        // Push the entry first and fill the strings in place, so a failing
        // allocation leaves every string already made reachable from `out`.
        out.push_back(kaia_cre_toc_entry{});
        kaia_cre_toc_entry &e = out.back();
        e.level = child->getLevel();
        e.page = child->getPage();
        e.title = dup_utf8(child->getName());
        e.xpointer = dup_utf8(child->getPath());
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
        static std::set<std::string> fonts;
        static std::string active_hyph_dir;
        if (registered) {
            *registered = 0;
        }
        if (!font_files || font_count == 0) {
            fail("CREngine needs at least one usable font; no font files supplied");
        }
        if (!fontMan && !InitFontManager(lString8())) {
            fail("cannot start the CREngine font manager");
        }
        int accepted = 0;
        for (size_t i = 0; i < font_count; i++) {
            if (!font_files[i]) continue;
            if (fonts.count(font_files[i]) || fontMan->RegisterFont(lString8(font_files[i]))) {
                fonts.insert(font_files[i]);
                accepted++;
            }
        }
        if (accepted == 0 || fontMan->GetFontCount() == 0) {
            fail("CREngine needs at least one usable font; none of the supplied files were accepted");
        }
        fontMan->RegularizeRegisteredFontsWeights();
        // Same storage limit as credocument.lua.
        setStorageMaxUncompressedSizeFactor(40.0f);
        std::string dir = hyph_dir ? hyph_dir : "";
        if (!dir.empty() && dir.back() != '/') dir += '/';
        if (dir != active_hyph_dir) {
            HyphMan::initDictionaries(lString32(dir.c_str()));
            active_hyph_dir = dir;
        }
        g_inited = true;
        if (registered) *registered = accepted;
        return KAIA_CRE_OK;
    });
}

int kaia_cre_init_with_data(const char *const *font_files, size_t font_count, const char *data_dir, int *registered) {
    // Build the path inside a guard so allocation failures stay behind the ABI.
    std::string hyph;
    int code = guard([&]() -> int {
        if (!data_dir || !*data_dir) fail("CREngine data directory is empty");
        hyph = data_dir;
        if (hyph.back() != '/') hyph += '/';
        hyph += "hyph";
        return KAIA_CRE_OK;
    });
    if (code != KAIA_CRE_OK) return code;
    return kaia_cre_init(font_files, font_count, hyph.c_str(), registered);
}

int kaia_cre_doc_new(int width, int height, kaia_cre_doc **out) {
    return guard([&]() -> int {
        if (width <= 0 || height <= 0 || !out) {
            fail("view size must be positive");
        }
        require_init();
        // Same constructor and mode as newDocView in base/cre.cpp. The page
        // header is off so the page holds only the text.
        LVDocView *view = new LVDocView(-1, true);
        kaia_cre_doc *doc = nullptr;
        try {
            view->setViewMode(DVM_PAGES, -1);
            view->Resize(width, height);
            view->setPageHeaderInfo(0);
            // The fallback values from readDefaults in base/cre.cpp, used when no
            // cr3.ini exists: autohinting (2) and harfbuzz kerning (3).
            CRPropRef props = LVCreatePropsContainer();
            props->setInt("font.hinting.mode", 2);
            props->setInt("font.kerning.mode", 3);
            // Match KOReader's txt_preformatted default before loading text.
            props->setInt("crengine.file.txt.preformatted", 1);
            view->propsApply(props);
            doc = new kaia_cre_doc{view};
        } catch (...) {
            // Nothing owns the view yet. Release it before the error leaves.
            try {
                view->close();
            } catch (...) {
            }
            delete view;
            throw;
        }
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
        require_init();
        LVDocView *view = view_of(doc);
        if (!face) {
            fail("font face is null");
        }
        // setDefaultFontFace drops the layout, so skip it when the face is
        // already set. Callers re-apply the face before each operation because
        // the preferred font below is process-wide.
        if (view->getDefaultFontFace() != lString8(face)) {
            view->setDefaultFontFace(lString8(face));
        }
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

int kaia_cre_set_font_weight(kaia_cre_doc *doc, int weight) {
    // KOReader's CreDocument:setFontBaseWeight writes the same property.
    // propsApply re-renders only when the value changes.
    return guard([&]() -> int { return set_int_property(doc, "font.face.base.weight", weight); });
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
        if (width > kMaxRenderSide || height > kMaxRenderSide) {
            fail("render size is too large");
        }
        // Checked multiplications: size_t is 32 bits on armv7. Each product
        // is also the size of a buffer used below.
        size_t pixels = 0;
        size_t need = 0;
        size_t bgra = 0;
        if (!mul_size(static_cast<size_t>(width), static_cast<size_t>(height), &pixels) ||
            !mul_size(pixels, color ? 3 : 1, &need) || !mul_size(pixels, 4, &bgra)) {
            fail("render size is too large");
        }
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
        std::vector<uint8_t> tmp(bgra, 0xFF);
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

int kaia_cre_range_text(kaia_cre_doc *doc, const char *start, const char *end, char **out) {
    return guard([&]() -> int {
        if (!start || !end || !out) fail("null argument");
        *out = nullptr;
        view_of(doc)->Render();
        ldomXPointer a = parse_xpointer(doc, start);
        ldomXPointer b = parse_xpointer(doc, end);
        if (a.isNull() || b.isNull()) return KAIA_CRE_NOT_FOUND;
        ldomXRange range(a, b);
        range.sort();
        // Match KOReader's getTextFromXPointers, including its single-char case.
        if (range.getStart() == range.getEnd() && range.getStart().isText()) {
            int offset = range.getEnd().getOffset();
            if (offset < range.getStart().getNode()->getText().length() - 1)
                range.getEnd().setOffset(offset + 1);
        }
        *out = dup_utf8(range.getRangeText('\n'));
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

int kaia_cre_compare_xpointers(kaia_cre_doc *doc, const char *a, const char *b, int32_t *out) {
    return guard([&]() -> int {
        if (!a || !b || !out) fail("xpointer comparison argument is null");
        ldomXPointerEx first = parse_xpointer(doc, a);
        ldomXPointerEx second = parse_xpointer(doc, b);
        if (first.isNull() || second.isNull()) return KAIA_CRE_NOT_FOUND;
        // KOReader returns positive when the second position follows the first.
        *out = second.compare(first);
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
        // Frees the hrefs made so far. Runs on every error path below.
        auto release = [&boxes]() {
            for (auto &b : boxes) {
                std::free(b.href);
                std::free(b.xpointer);
                std::free(b.note);
            }
            boxes.clear();
        };
        try {
            for (int i = 0; i < links.length(); i++) {
                ldomXPointer a_xpointer;
                lString32 href = links[i]->getHRef(a_xpointer);
                int32_t target = -1;
                lString32 xpointer;
                lString32 note;
                if (!href.empty() && href[0] == '#') {
                    ldomNode *node = dom->getElementById(href.c_str() + 1);
                    if (node) {
                        target = view->getBookmarkPage(ldomXPointer(node, 0), false);
                        xpointer = ldomXPointer(node, 0).toString();
                        if (!a_xpointer.isNull()) {
                            FootnoteDocument context{view, dom};
                            lString32 reason, stop;
                            ldomXRange extended;
                            // readerlink.lua defaults: trusted source, semantic notes,
                            // FB2 notes, forward anchors, source shape and 10,000 chars.
                            constexpr int flags = 0x0002 | 0x0004 | 0x0008 | 0x0010
                                | 0x0020 | 0x0040 | 0x0100 | 0x0200 | 0x0400 | 0x0800
                                | 0x1000 | 0x4000 | 0x8000;
                            bool footnote = _isLinkToFootnote(&context, a_xpointer.toString(),
                                href, flags, 10000, reason, stop, extended);
                            // EPUB also permits an aside without an epub:type.
                            if (node->getNodeName() == "aside") footnote = true;
                            if (footnote) {
                                if (!extended.isNull()) {
                                    note = extended.getRangeText('\n');
                                } else {
                                    ldomNode *container = ldomXPointerEx(node, 0).getFinalNode();
                                    note = (container ? container : node)->getText('\n');
                                }
                            }
                        }
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
                    // Push the box first, then fill the href in place, so the
                    // vector owns the string as soon as it exists.
                    boxes.push_back(kaia_cre_link_box{});
                    kaia_cre_link_box &b = boxes.back();
                    b.x0 = static_cast<float>(rc.left);
                    b.y0 = static_cast<float>(rc.top);
                    b.x1 = static_cast<float>(rc.right);
                    b.y1 = static_cast<float>(rc.bottom);
                    b.page = target;
                    b.href = dup_utf8(href);
                    b.xpointer = xpointer.empty() ? nullptr : dup_utf8(xpointer);
                    b.note = note.empty() ? nullptr : dup_utf8(note);
                }
            }
            if (boxes.empty()) {
                return KAIA_CRE_OK;
            }
            kaia_cre_link_box *arr =
                static_cast<kaia_cre_link_box *>(std::malloc(sizeof(kaia_cre_link_box) * boxes.size()));
            if (!arr) {
                fail("out of memory");
            }
            std::memcpy(arr, boxes.data(), sizeof(kaia_cre_link_box) * boxes.size());
            *count = boxes.size();
            *out = arr;
            return KAIA_CRE_OK;
        } catch (...) {
            release();
            throw;
        }
    });
}

void kaia_cre_free_link_boxes(kaia_cre_link_box *boxes, size_t count) {
    if (!boxes) {
        return;
    }
    for (size_t i = 0; i < count; i++) {
        std::free(boxes[i].href);
        std::free(boxes[i].xpointer);
        std::free(boxes[i].note);
    }
    std::free(boxes);
}

namespace {

// Sums the area of the image elements in a range that lie inside the page box.
class ImageArea : public ldomNodeCallback {
public:
    ImageArea(int top, int bottom, int right) : top_(top), bottom_(bottom), right_(right) {}
    double area() const { return area_; }
    void onText(ldomXRange *) override {}
    bool onElement(ldomXPointerEx *ptr) override {
        ldomNode *node = ptr->getNode();
        if (!node) {
            return true;
        }
        // The same element names as CREngine's own image count.
        lString32 name = node->getNodeName();
        if (name != "img" && name != "image") {
            return true;
        }
        // The element's own rect. getAbsRect has no box for an image, so the
        // pointer to its first position gives the drawn box.
        lvRect rc;
        if (!ldomXPointer(node, 0).getRectEx(rc)) {
            return true;
        }
        int x0 = std::max(rc.left, 0);
        int x1 = std::min(rc.right, right_);
        int y0 = std::max(rc.top, top_);
        int y1 = std::min(rc.bottom, bottom_);
        if (x1 > x0 && y1 > y0) {
            area_ += static_cast<double>(x1 - x0) * static_cast<double>(y1 - y0);
        }
        return true;
    }

private:
    int top_;
    int bottom_;
    int right_;
    double area_ = 0.0;
};

}  // namespace

int kaia_cre_page_image_share(kaia_cre_doc *doc, float *out) {
    return guard([&]() -> int {
        if (!out) {
            fail("null output");
        }
        *out = 0.0f;
        LVDocView *view = view_of(doc);
        view->Render();
        // The page box is only known in paged mode with one visible page.
        if (view->getViewMode() != DVM_PAGES || view->getVisiblePageCount() != 1) {
            return KAIA_CRE_NOT_FOUND;
        }
        int width = view->GetWidth();
        int height = view->GetHeight();
        int page = view->getCurPage(true);
        if (width <= 0 || height <= 0 || page < 0) {
            return KAIA_CRE_NOT_FOUND;
        }
        // Document y of the page box. Images are measured in document units.
        int top = view->getPageStartY(page);
        int bottom = top + view->getPageHeight(page);
        ImageArea counter(top, bottom, width);
        LVRef<ldomXRange> range = view->getPageDocumentRange(-1);
        if (!range.isNull()) {
            range->forEach(&counter);
        }
        double share = counter.area() / (static_cast<double>(width) * static_cast<double>(height));
        *out = static_cast<float>(std::min(share, 1.0));
        return KAIA_CRE_OK;
    });
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
        *count = 0;
        *out = nullptr;
        if (entries.empty()) {
            return KAIA_CRE_OK;
        }
        kaia_cre_toc_entry *arr =
            static_cast<kaia_cre_toc_entry *>(std::malloc(sizeof(kaia_cre_toc_entry) * entries.size()));
        if (!arr) {
            for (auto &e : entries) {
                std::free(e.title);
                std::free(e.xpointer);
            }
            fail("out of memory");
        }
        std::memcpy(arr, entries.data(), sizeof(kaia_cre_toc_entry) * entries.size());
        *count = entries.size();
        *out = arr;
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

int kaia_cre_font_families(kaia_cre_font_family **out, size_t *count) {
    return guard([&]() -> int {
        if (!out || !count) {
            fail("null output");
        }
        require_init();
        *out = nullptr;
        *count = 0;
        lString32Collection names;
        fontMan->getFaceList(names);
        std::vector<kaia_cre_font_family> entries;
        try {
            for (int i = 0; i < names.length(); i++) {
                lString8 file;
                int face_index = 0;
                int family_type = 0;
                bool has_ot_math = false;
                bool has_emojis = false;
                if (!fontMan->getFontFileNameAndFaceIndex(names[i], false, false, file, face_index,
                                                          family_type, has_ot_math, has_emojis)) {
                    continue;
                }
                kaia_cre_font_family entry{dup_utf8(names[i]), nullptr};
                try {
                    entry.file = dup_utf8(std::string(file.c_str()));
                    entries.push_back(entry);
                } catch (...) {
                    free_strings(entry.family, entry.file);
                    throw;
                }
            }
        } catch (...) {
            for (auto &e : entries) {
                free_strings(e.family, e.file);
            }
            throw;
        }
        if (entries.empty()) {
            return KAIA_CRE_OK;
        }
        kaia_cre_font_family *arr =
            static_cast<kaia_cre_font_family *>(std::malloc(sizeof(kaia_cre_font_family) * entries.size()));
        if (!arr) {
            for (auto &e : entries) {
                free_strings(e.family, e.file);
            }
            fail("out of memory");
        }
        std::memcpy(arr, entries.data(), sizeof(kaia_cre_font_family) * entries.size());
        *count = entries.size();
        *out = arr;
        return KAIA_CRE_OK;
    });
}

void kaia_cre_free_font_families(kaia_cre_font_family *families, size_t count) {
    if (!families) {
        return;
    }
    for (size_t i = 0; i < count; i++) {
        free_strings(families[i].family, families[i].file);
    }
    std::free(families);
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
        for (long steps = 0; cur.compare(last) < 0 && steps < kMaxSteps; steps++) {
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

int kaia_cre_first_char_box(kaia_cre_doc *doc, const char *xp, kaia_cre_char_box *out) {
    return guard([&]() -> int {
        if (!out || !xp) {
            fail("null argument");
        }
        LVDocView *view = view_of(doc);
        view->Render();
        ldomDocument *dom = dom_of(doc);
        ldomXPointer start = dom->createXPointer(lString32(xp));
        if (start.isNull()) {
            return KAIA_CRE_NOT_FOUND;
        }
        // Same walk as kaia_cre_char_boxes, but it stops at the first character
        // on the page. A walk to the end of the document costs time in proportion
        // to the book size.
        ldomXPointerEx cur(start);
        ldomXPointerEx last(dom->getRootNode(), dom->getRootNode()->getChildCount());
        ldomNode *text_node = nullptr;
        lString32 text;
        const long kMaxSteps = 2000000;
        for (long steps = 0; cur.compare(last) < 0 && steps < kMaxSteps; steps++) {
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
                    out->x0 = static_cast<float>(rc.left);
                    out->y0 = static_cast<float>(rc.top);
                    out->x1 = static_cast<float>(rc.right);
                    out->y1 = static_cast<float>(rc.bottom);
                    out->line = 0;
                    out->codepoint = static_cast<uint32_t>(text[off]);
                    return KAIA_CRE_OK;
                }
            }
            if (!cur.nextVisibleChar()) {
                break;
            }
        }
        return KAIA_CRE_NOT_FOUND;
    });
}

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
        auto release = [&ranges]() {
            for (auto &r : ranges) {
                free_strings(r.start, r.end);
            }
            ranges.clear();
        };
        try {
            for (int i = 0; i < hits.length(); i++) {
                // Push first and fill in place, so the vector owns each string
                // as soon as it exists.
                ranges.push_back(kaia_cre_range{});
                kaia_cre_range &r = ranges.back();
                r.start = dup_utf8(hits[i]->getStart().toString());
                r.end = dup_utf8(hits[i]->getEnd().toString());
            }
            if (ranges.empty()) {
                return KAIA_CRE_OK;
            }
            kaia_cre_range *arr = static_cast<kaia_cre_range *>(std::malloc(sizeof(kaia_cre_range) * ranges.size()));
            if (!arr) {
                fail("out of memory");
            }
            std::memcpy(arr, ranges.data(), sizeof(kaia_cre_range) * ranges.size());
            *count = ranges.size();
            *out = arr;
            return KAIA_CRE_OK;
        } catch (...) {
            release();
            throw;
        }
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
        }
        if (authors) {
            *authors = nullptr;
        }
        if (language) {
            *language = nullptr;
        }
        try {
            if (title) {
                lString32 v = view->getTitle();
                if (!v.empty()) {
                    *title = dup_utf8(v);
                }
            }
            if (authors) {
                lString32 v = view->getAuthors();
                if (!v.empty()) {
                    *authors = dup_utf8(v);
                }
            }
            if (language) {
                lString32 v = view->getLanguage();
                if (!v.empty()) {
                    *language = dup_utf8(v);
                }
            }
        } catch (...) {
            // Report no strings on failure, so the caller has nothing to free.
            if (title) {
                std::free(*title);
                *title = nullptr;
            }
            if (authors) {
                std::free(*authors);
                *authors = nullptr;
            }
            if (language) {
                std::free(*language);
                *language = nullptr;
            }
            throw;
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

int kaia_cre_text_range(kaia_cre_doc *doc, const char *start, const char *end, char **out) {
    return guard([&]() -> int {
        if (!start || !end || !out) fail("null text range argument");
        *out = nullptr;
        auto a = parse_xpointer(doc, start);
        auto b = parse_xpointer(doc, end);
        if (a.isNull() || b.isNull()) return KAIA_CRE_NOT_FOUND;
        ldomXRange range(a, b);
        range.sort();
        *out = dup_utf8(range.getRangeText('\n'));
        return KAIA_CRE_OK;
    });
}

int kaia_cre_word_boundary(kaia_cre_doc *doc, const char *xp, int next, char **out) {
    return guard([&]() -> int {
        if (!xp || !out) fail("null word boundary argument");
        *out = nullptr;
        ldomXPointerEx p = parse_xpointer(doc, xp);
        if (p.isNull() || !(next ? p.nextVisibleWordEnd() : p.prevVisibleWordStart()))
            return KAIA_CRE_NOT_FOUND;
        *out = dup_utf8(p.toString());
        return KAIA_CRE_OK;
    });
}

void kaia_cre_free_text_matches(kaia_cre_text_match *items, size_t count) {
    if (!items) return;
    for (size_t i = 0; i < count; ++i)
        for (auto field : items[i].fields) std::free(field);
    std::free(items);
}

int kaia_cre_find_all(kaia_cre_doc *doc, const char *pattern, int insensitive, int regex,
        int context, int max_hits, kaia_cre_text_match **out, size_t *count) {
    return guard([&]() -> int {
        if (!pattern || !out || !count) fail("null search argument");
        *out = nullptr;
        *count = 0;
        view_of(doc)->Render();
        ldomXRangeList matches;
        if (!*pattern || max_hits <= 0) return KAIA_CRE_OK;
        dom_of(doc)->findText(lString32(pattern), insensitive != 0, false, -1, -1,
            matches, max_hits, -1, -1, regex != 0, 0);
        size_t n = matches.length();
        auto items = static_cast<kaia_cre_text_match *>(std::calloc(n, sizeof(kaia_cre_text_match)));
        if (n && !items) fail("out of memory");
        try {
            for (size_t i = 0; i < n; ++i) {
                auto range = matches[i];
                auto &f = items[i].fields;
                f[0] = dup_utf8(range->getStart().toString());
                f[1] = dup_utf8(range->getEnd().toString());
                f[2] = dup_utf8(range->getRangeText('\n'));
                ldomXPointerEx start = range->getStart();
                ldomXPointerEx end = range->getEnd();
                if (!start.isVisibleWordStart()) {
                    start.prevVisibleWordStart();
                    ldomXRange prefix(start, range->getStart());
                    f[3] = dup_utf8(prefix.getRangeText('\n'));
                    f[4] = dup_utf8(start.toString());
                }
                if (!end.isVisibleWordEnd()) {
                    end.nextVisibleWordEnd();
                    ldomXRange suffix(range->getEnd(), end);
                    f[5] = dup_utf8(suffix.getRangeText('\n'));
                    f[6] = dup_utf8(end.toString());
                }
                if (context > 0) {
                    auto prev = start;
                    for (int j = 0; j < context && prev.prevVisibleWordStart(); ++j) {}
                    ldomXRange before(prev, start);
                    f[7] = dup_utf8(before.getRangeText('\n'));
                    auto next = end;
                    auto tmp = end;
                    for (int j = 0; j < context; ++j) {
                        if ((j == 0 && end.getNode()->isRoot()) || !tmp.nextVisibleWordEnd()) break;
                        next = tmp;
                    }
                    ldomXRange after(end, next);
                    f[8] = dup_utf8(after.getRangeText('\n'));
                }
            }
        } catch (...) {
            kaia_cre_free_text_matches(items, n);
            throw;
        }
        *out = items;
        *count = n;
        return KAIA_CRE_OK;
    });
}
