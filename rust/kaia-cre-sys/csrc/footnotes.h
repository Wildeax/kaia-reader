/*
    KindlePDFViewer: CREngine abstraction for Lua
    Copyright (C) 2012 Hans-Werner Hilse <hilse@web.de>
                       Qingping Hou <qingping.hou@gmail.com>

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program.  If not, see <http://www.gnu.org/licenses/>.
*/

// Adapted from _isLinkToFootnote in base/cre.cpp, KOReader v2026.07.1.
// Keep the detection order and range extension in step with readerlink.lua.
struct FootnoteDocument {
    LVDocView *text_view;
    ldomDocument *dom_doc;
};

static bool _isLinkToFootnote(FootnoteDocument *doc, const lString32 source_xpointer, const lString32 target_xpointer,
            const int flags, const int maxTextSize, lString32 &reason,
            lString32 &extendedStopReason, ldomXRange &extendedRange)
{
    const ldomXPointerEx sourceXP = ldomXPointerEx(doc->dom_doc->createXPointer(source_xpointer));
    const ldomXPointerEx targetXP = ldomXPointerEx(doc->dom_doc->createXPointer(target_xpointer));
    if (sourceXP.isNull() || targetXP.isNull()) return false;
    ldomNode *sourceNode = sourceXP.getNode();
    ldomNode *targetNode = targetXP.getNode();
    const lString32 targetXpath = ldomXPointer(targetNode, 0).toString();
    reason = "";
    extendedStopReason = "trusted container, no need";
    bool likelyFootnote = false;
    if (flags & 0x0001) {
        likelyFootnote = true;
    }
    bool trusted_source_xpointer = flags & 0x0002;
    if (trusted_source_xpointer) {
        css_style_ref_t sourceNodeStyle = sourceNode->getStyle();
        if ( STYLE_HAS_CR_HINT(sourceNodeStyle, NOTEREF) ) {
            reason = "source has -cr-hint: noteref";
            return true;
        }
        else if ( STYLE_HAS_CR_HINT(sourceNodeStyle, NOTEREF_IGNORE) ) {
            reason = "source has -cr-hint: noteref-ignore";
            return false;
        }
    }
    css_style_ref_t targetNodeStyle = targetNode->getStyle();
    if ( STYLE_HAS_CR_HINT(targetNodeStyle, FOOTNOTE) ) {
        reason = "target has -cr-hint: footnote";
        return true;
    }
    else if ( STYLE_HAS_CR_HINT(targetNodeStyle, FOOTNOTE_IGNORE) ) {
        reason = "target has -cr-hint: footnote-ignore";
        return false;
    }
    if (flags & 0x0004) {
        if (trusted_source_xpointer) {
            lString32 type = sourceNode->getAttributeValue("epub", "type");
            if (type.empty()) {
                type = sourceNode->getAttributeValue("type");
            }
            if (!type.empty()) {
                type.lowercase();
                lString32Collection types;
                types.parse(type, ' ', true);
                for (int i=0; i<types.length(); i++) {
                    lString32 type = types[i];
                    if (type == "noteref") {
                        reason = "source has epub:type=" + type;
                        return true;
                    }
                    if (type == "link") {
                        reason = "source has epub:type=" + type;
                        return false;
                    }
                }
            }
            lString32 role = sourceNode->getAttributeValue("role");
            if (!role.empty()) {
                role.lowercase();
                lString32Collection roles;
                roles.parse(role, ' ', true);
                for (int i=0; i<roles.length(); i++) {
                    lString32 role = roles[i];
                    if (role == "doc-noteref") {
                        reason = "source has role=" + role;
                        return true;
                    }
                    if (role == "doc-link") {
                        reason = "source has role=" + role;
                        return false;
                    }
                }
            }
        }
        lString32 type = targetNode->getAttributeValue("epub", "type");
        if (type.empty()) {
            type = targetNode->getAttributeValue("type");
        }
        if (!type.empty()) {
            type.lowercase();
            lString32Collection types;
            types.parse(type, ' ', true);
            for (int i=0; i<types.length(); i++) {
                lString32 type = types[i];
                if (type == "note" || type == "footnote" || type == "rearnote" || type == "endnote") {
                    reason = "target has epub:type=" + type;
                    return true;
                }
            }
        }
        lString32 role = targetNode->getAttributeValue("role");
        if (!role.empty()) {
            lString32Collection roles;
            roles.parse(role, ' ', true);
            for (int i=0; i<roles.length(); i++) {
                lString32 role = roles[i];
                if (role == "doc-note" || role == "doc-footnote" || role == "doc-rearnote" || role == "doc-endnote") {
                    reason = "target has role=" + role;
                    return true;
                }
            }
        }
    }
    if (flags & 0x0008) {
        if ( targetNode->getNodeId() == doc->dom_doc->getElementNameIndex("section") ) {
            lUInt16 el_body = doc->dom_doc->getElementNameIndex("body");
            ldomNode * n = targetNode->getParentNode();
            while ( n && !n->isNull() ) {
                if ( n->getNodeId() == el_body ) {
                    lString32 name = n->getAttributeValue("name");
                    if (!name.empty()) {
                        name.lowercase();
                        if (name == "notes") {
                            reason = "target is FB2 footnote (body[name=notes] section)";
                            return true;
                        }
                        if (name == "comments") {
                            reason = "target is FB2 footnote (body[name=comments] section)";
                            return true;
                        }
                    }
                    break;
                }
                n = n->getParentNode();
            }
        }
    }
    if (flags & 0x0010) {
        if ( target_xpointer[0] != '#' ) {
            reason = "target is not an internal link (not #something)";
            return false;
        }
        if ( target_xpointer.pos("_ ") < 0 ) {
            reason = "target is missing an anchor (#someId)";
            return false;
        }
    }
    if (flags & 0x0020) {
        if (sourceXP.compare(targetXP) > 0) {
            reason = "target does not come after source in book";
            return false;
        }
    }
    if (flags & 0x0040) {
        LVTocItem * item = doc->text_view->getToc();
        if (item->getChildCount() > 0) {
            int nextChildIndex = 0;
            item = item->getChild(nextChildIndex);
            while (true) {
                if (nextChildIndex == 0) {
                    if (item->getPath() == targetXpath) {
                        reason = "target also appears in TOC";
                        return false;
                    }
                }
                if (nextChildIndex < item->getChildCount()) {
                    item = item->getChild(nextChildIndex);
                    nextChildIndex = 0;
                    continue;
                }
                nextChildIndex = item->getIndex() + 1;
                item = item->getParent();
                if (!item)
                    break;
            }
        }
    }
    if (flags & 0x0100 && trusted_source_xpointer) {
        ldomNode * finalNode = sourceXP.getFinalNode();
        lString32 sourceText = sourceNode->getText();
        if ( sourceText.empty() ) {
            ldomXPointerEx sourceEndXP = sourceXP;
            while (true) {
                if ( sourceEndXP.nextSibling() )
                    break;
                if ( !sourceEndXP.parent() )
                    break;
            }
            ldomXRange fullNodeRange(sourceXP, sourceEndXP);
            sourceText = fullNodeRange.getRangeText( '\n', 'Z');
            if ( sourceText.empty() ) {
                reason = "source has no text nor image content";
                return false;
            }
        }
        if ( finalNode && !finalNode->isNull() ) {
            if ( sourceText == finalNode->getText() ) {
                reason = "source is the only content of its parent block";
                return false;
            }
        }
    }
    if (flags & 0x0200 && trusted_source_xpointer && !likelyFootnote) {
        bool seen_non_empty_text = false;
        bool is_only_footnote_likely_vertical_align = true;
        ldomXPointerEx endText = sourceXP;
        endText.lastInnerTextNode(true);
        ldomXPointerEx curText = sourceXP;
        ldomNode * finalNode = sourceXP.getFinalNode();
        while ( curText.nextText() && curText.compare(endText) <= 0 ) {
            lString32 nodeText = curText.getText();
            int textLen = nodeText.length();
            if ( textLen == 0 || (textLen == 1 && nodeText[0] == ' ' ) )
                continue;
            seen_non_empty_text = true;
            bool is_vertically_shifted = false;
            ldomNode * parent = curText.getNode()->getParentNode();
            while (parent && !parent->isNull()) {
                css_style_ref_t style = parent->getStyle();
                css_length_t vertical_align = style->vertical_align;
                if (vertical_align.type == css_val_unspecified) {
                    css_vertical_align_t va = (css_vertical_align_t)vertical_align.value;
                    if (va != css_va_inherit && va != css_va_baseline) {
                        is_vertically_shifted = true;
                        break;
                    }
                }
                else {
                    if (vertical_align.value != 0) {
                        is_vertically_shifted = true;
                        break;
                    }
                }
                if (parent == finalNode)
                    break;
                parent = parent->getParentNode();
            }
            if ( !is_vertically_shifted ) {
                is_only_footnote_likely_vertical_align = false;
                break;
            }
        }
        if ( seen_non_empty_text && is_only_footnote_likely_vertical_align ) {
            if ( !reason.empty() ) reason += "; ";
            reason += "source has all content shifted by vertical-align";
            likelyFootnote = true;
        }
        else {
            lUInt16 el_sup = doc->dom_doc->getElementNameIndex("sup");
            lUInt16 el_sub = doc->dom_doc->getElementNameIndex("sub");
            ldomNode * n = sourceNode->getParentNode();
            for ( int i=0; i<2; i++ ) {
                if ( !n || n->isNull() )
                    break;
                if ( n->getNodeId() == el_sup || n->getNodeId() == el_sub ) {
                    if ( !reason.empty() ) reason += "; ";
                    reason += "source is child or grandchild of <sup> or <sub>";
                    likelyFootnote = true;
                    break;
                }
                n = n->getParentNode();
            }
            if ( !likelyFootnote ) {
                bool hasOnlySupSub = false;
                for ( int i=0; i<sourceNode->getChildCount(); i++ ) {
                    n = sourceNode->getChildNode(i);
                    if ( n->isText() ) {
                        lString32 nodeText = n->getText();
                        int textLen = nodeText.length();
                        if ( textLen == 0 || (textLen == 1 && nodeText[0] == ' ' ) ) {
                            continue;
                        }
                        else {
                            hasOnlySupSub = false;
                            break;
                        }
                    }
                    else {
                        if ( n->getNodeId() == el_sup || n->getNodeId() == el_sub ) {
                            hasOnlySupSub = true;
                        }
                        else {
                            hasOnlySupSub = false;
                            break;
                        }
                    }
                }
                if ( hasOnlySupSub ) {
                    if ( !reason.empty() ) reason += "; ";
                    reason += "source has only <sup> or <sub> children";
                    likelyFootnote = true;
                }
            }
        }
    }
    if ( (flags & 0x0400 || flags & 0x0800) && trusted_source_xpointer && !likelyFootnote) {
        lString32 sourceText = sourceNode->getText();
        int nbDigits = 0;
        int nbAlpha = 0;
        int nbOthers = 0;
        int nbRomans = 0;
        for (int i=0 ; i<sourceText.length(); i++) {
            if ( !lStr_isWordSeparator(sourceText[i]) ) {
                int props = lGetCharProps(sourceText[i]);
                if (props & CH_PROP_DIGIT)
                    nbDigits += 1;
                else if (props & CH_PROP_ALPHA) {
                    nbAlpha += 1;
                    lChar32 c = sourceText[i];
                    if (c == 'i' || c == 'v' || c == 'x' || c == 'l' || c == 'c' || c == 'd' || c == 'm' ||
                        c == 'I' || c == 'V' || c == 'X' || c == 'L' || c == 'C' || c == 'D' || c == 'M' ) {
                            nbRomans += 1;
                    }
                }
                else
                    nbOthers += 1;
            }
        }
        if (flags & 0x0400 && nbDigits >= 1 && nbDigits <= 3 && nbAlpha==0 && nbOthers==0) {
            if (!reason.empty()) reason += "; ";
            reason += "source text is only 1 to 3 digits";
            likelyFootnote = true;
        }
        if (flags & 0x0800 && nbAlpha >= 1 && nbAlpha <= 2 && nbDigits >= 0 && nbDigits <= 2 && nbOthers==0) {
            if (!reason.empty()) reason += "; ";
            reason += "source text is 1 to 2 letters with 0 to 2 digits";
            likelyFootnote = true;
        }
        else if (flags & 0x0800 && nbRomans >= 1 && nbRomans <= 10 && nbRomans == nbAlpha && nbDigits==0 && nbOthers==0) {
            if (!reason.empty()) reason += "; ";
            reason += "source text is 1 to 10 roman numerals";
            likelyFootnote = true;
        }
    }
    if (flags & 0x1000) {
        lUInt16 el_h1 = doc->dom_doc->getElementNameIndex("h1");
        lUInt16 el_h6 = doc->dom_doc->getElementNameIndex("h6");
        ldomNode * n = targetNode;
        while ( n && !n->isNull() ) {
            if ( n->getNodeId() >= el_h1 && n->getNodeId() <= el_h6 ) {
                reason = "target is, or is inside, a <h1>...<h6>";
                return false;
            }
            n = n->getParentNode();
        }
        ldomXPointerEx curXP = targetXP;
        ldomXPointerEx endXP = targetXP;
        endXP.lastInnerNode();
        while (!curXP.isNull() && curXP.compare(endXP) <= 0) {
            lUInt16 id = curXP.getNode()->getNodeId();
            if ( id >= el_h1 && id <= el_h6 ) {
                reason = "target contains a <h1>...<h6>";
                return false;
            }
            if (!curXP.nextElement())
                break;
        }
    }
    if (flags & 0x4000) {
        bool do_extend = true;
        ldomXPointerEx extendedStart;
        ldomXPointerEx curPos;
        ldomNode * firstFinalNode = targetXP.getFinalNode();
        if (firstFinalNode && !firstFinalNode->isNull()) {
            extendedStart = ldomXPointerEx(firstFinalNode, 0);
            curPos = extendedStart;
            do_extend = curPos.nextOuterElement();
        }
        else {
            curPos = targetXP;
            ldomXPointerEx endPos = curPos;
            while ( endPos.lastChild() ) {}
            bool has_final_child = false;
            while ( curPos.nextElement() && curPos.compare(endPos) <= 0 ) {
                if ( curPos.isFinalNode() ) {
                    has_final_child = true;
                    extendedStopReason = "contains 1 or more final nodes, trusting container";
                    do_extend = false;
                    break;
                }
            }
            if (!has_final_child) {
                extendedStart = targetXP;
                curPos = targetXP;
                do_extend = false;
                if ( curPos.nextOuterElement() ) {
                    do {
                        if ( curPos.isFinalNode() ) {
                            do_extend = curPos.nextOuterElement();
                            break;
                        }
                    }
                    while ( curPos.nextElement() );
                }
            }
        }
        if (do_extend) {
            lUInt16 el_DocFragment = doc->dom_doc->getElementNameIndex("DocFragment");
            lUInt16 el_body = doc->dom_doc->getElementNameIndex("body");
            lUInt16 el_h1 = doc->dom_doc->getElementNameIndex("h1");
            lUInt16 el_h6 = doc->dom_doc->getElementNameIndex("h6");
            lUInt16 el_a = doc->dom_doc->getElementNameIndex("a");
            ldomNode * goodFinalNode = NULL;
            ldomNode * curFinalNode = NULL;
            ldomXPointerEx notAfter;
            lString32 extStopReason;
            extStopReason = "End of document met";
            while (true) {
                ldomNode * newFinalNode = curPos.getFinalNode();
                if (newFinalNode != curFinalNode) {
                    if (curFinalNode && !curFinalNode->isNull())
                        goodFinalNode = curFinalNode;
                    curFinalNode = newFinalNode;
                }
                ldomNode * node = curPos.getNode();
                lUInt16 nodeId = node->getNodeId();
                if ( nodeId == el_body || nodeId == el_DocFragment ) {
                    extStopReason = "end of document fragment met";
                    break;
                }
                if ( nodeId >= el_h1 && nodeId <= el_h6 ) {
                    extStopReason = "H1..H6 met";
                    break;
                }
                css_style_ref_t style = node->getStyle();
                css_page_break_t pb_before = style->page_break_before;
                css_page_break_t pb_after = style->page_break_after;
                if ( pb_before == css_pb_always || pb_before == css_pb_left || pb_before == css_pb_right ) {
                    extStopReason = "page-break-before met";
                    break;
                }
                if ( pb_after == css_pb_always || pb_after == css_pb_left || pb_after == css_pb_right ) {
                    ldomXPointerEx tmpPos = curPos;
                    if ( tmpPos.nextOuterElement() ) {
                        notAfter = tmpPos;
                    }
                }
                lString32 id = node->getAttributeValue("id");
                if ( !id.empty() ) {
                    extStopReason = "node with 'id=' attr met";
                    break;
                }
                else if ( nodeId == el_a ) {
                    lString32 name = node->getAttributeValue("name");
                    if ( !name.empty() ) {
                        extStopReason = "node A with 'name=' attr met";
                        break;
                    }
                }
                if ( !curPos.nextElement() ) {
                    extStopReason = "end of document met";
                    break;
                }
                if ( !notAfter.isNull() && curPos.compare(notAfter) >= 0 ) {
                    extStopReason = "page-break-after met";
                    if ( curFinalNode && !curFinalNode->isNull() )
                        goodFinalNode = curFinalNode;
                    break;
                }
            }
            extendedStopReason = extStopReason;
            if ( goodFinalNode && !goodFinalNode->isNull() ) {
                ldomXPointerEx extendedEnd = ldomXPointerEx(goodFinalNode, 0);
                extendedEnd.lastInnerNode(true);
                extendedRange = ldomXRange(extendedStart, extendedEnd);
            }
        }
    }
    int size = 0;
    int hasContent = false;
    ldomXPointerEx curText;
    ldomXPointerEx endText;
    if ( !extendedRange.isNull() ) {
        curText = extendedRange.getStart();
        endText = extendedRange.getEnd();
    }
    else {
        ldomNode * finalNode = targetXP.getFinalNode();
        if ( finalNode && !finalNode->isNull() )
            curText = ldomXPointerEx(finalNode, 0);
        else
            curText = targetXP;
        endText = curText;
        endText.lastInnerTextNode();
    }
    while (curText.nextText() && curText.compare(endText) <= 0) {
        lString32 nodeText = curText.getText();
        size += nodeText.length();
        if ( !hasContent ) {
            hasContent = !(nodeText.trim().empty());
        }
        if (flags & 0x8000) {
            if (size > maxTextSize) {
                reason = "target text is too large";
                return false;
            }
        }
        else if ( hasContent ) {
            break;
        }
    }
    if ( !hasContent ) {
        reason = "target text is empty or only spaces";
        return false;
    }
    if ( likelyFootnote ) {
        if ( reason.empty() ) {
            reason = "no decision, default to be a footnote";
        }
        return true;
    }
    if ( !reason.empty() ) reason += "; ";
    reason += "no decision made, default to not a footnote";
    return false;
}
