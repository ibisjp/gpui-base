// Modified by ibis inc.: this file is new (not part of gpui-base 0.6.6 as published). Apache-2.0, like the rest of the crate.
//! The underline of the text being composed by an input method (ibisOffice GitHub issue #138).
//!
//! The platform says how each clause of the marked text is underlined (`gpui::ImeClause`: thick or thin; solid, dotted or wavy; UTF-16
//! units from the start of the marked text). These pure functions turn that into byte ranges of the marked text and into the line the
//! input draws. When the platform says nothing (Linux, the web) every character gets the thin solid line the input always drew.

use std::ops::Range;

use gpui::{Hsla, ImeClause, ImeLine, Pixels, UnderlineStyle, px};

/// A run of the marked text (bytes of the marked text) with one underline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImeSegment {
    pub range_start: usize,
    pub range_end: usize,
    pub thick: bool,
    pub line: ImeLine,
}

impl ImeSegment {
    pub(crate) fn range(&self) -> Range<usize> {
        self.range_start..self.range_end
    }

    pub(crate) fn thickness(&self) -> Pixels {
        if self.thick { px(2.) } else { px(1.) }
    }

    /// The underline gpui paints with the text; `None` for the dotted line, which gpui cannot draw (the input paints the dots itself).
    pub(crate) fn text_underline(&self, color: Hsla) -> Option<UnderlineStyle> {
        match self.line {
            ImeLine::Solid | ImeLine::Wavy => Some(UnderlineStyle {
                thickness: self.thickness(),
                color: Some(color),
                wavy: self.line == ImeLine::Wavy,
            }),
            ImeLine::Dotted => None,
        }
    }
}

/// The segments of `marked_text` (bytes) from the clauses the platform gave (UTF-16 units). Characters no clause covers, and all of them
/// when `clauses` is empty, get a thin solid line. Neighbouring segments with the same line are one.
pub(crate) fn ime_segments(marked_text: &str, clauses: &[ImeClause]) -> Vec<ImeSegment> {
    // Byte offset of the first character starting at or after each UTF-16 offset.
    let mut units = Vec::with_capacity(marked_text.len() + 1);
    let mut u = 0;
    for (byte, c) in marked_text.char_indices() {
        units.push((u, byte));
        u += c.len_utf16();
    }
    units.push((u, marked_text.len()));
    let to_byte = |u: usize| units[units.partition_point(|&(x, _)| x < u).min(units.len() - 1)].1;
    let len = marked_text.len();

    let mut out: Vec<ImeSegment> = Vec::new();
    let mut push = |range: Range<usize>, thick: bool, line: ImeLine| {
        if range.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.thick == thick && last.line == line && last.range_end == range.start => last.range_end = range.end,
            _ => out.push(ImeSegment { range_start: range.start, range_end: range.end, thick, line }),
        }
    };
    let mut at = 0;
    let mut sorted: Vec<&ImeClause> = clauses.iter().collect();
    sorted.sort_by_key(|c| c.range.start);
    for c in sorted {
        let (a, b) = (to_byte(c.range.start).max(at), to_byte(c.range.end).min(len));
        if a >= b {
            continue;
        }
        push(at..a, false, ImeLine::Solid);
        push(a..b, c.thick, c.line);
        at = b;
    }
    push(at..len, false, ImeLine::Solid);
    out
}

/// Cut `run` (covering `run_range` of the text) at the segments, each piece carrying its segment's underline (`None` for dotted and for
/// the part no segment covers).
pub(crate) fn split_run_for_ime_segments(
    run: gpui::TextRun,
    run_range: Range<usize>,
    segments: &[(Range<usize>, Option<UnderlineStyle>)],
) -> smallvec::SmallVec<[gpui::TextRun; 3]> {
    if run.len == 0 {
        return smallvec::SmallVec::new();
    }
    let mut out = smallvec::SmallVec::new();
    let mut cursor = run_range.start;
    for (seg, underline) in segments {
        let start = seg.start.max(cursor);
        let end = seg.end.min(run_range.end);
        if start >= end {
            continue;
        }
        if start > cursor {
            out.push(gpui::TextRun { len: start - cursor, ..run.clone() });
        }
        out.push(gpui::TextRun { len: end - start, underline: *underline, ..run.clone() });
        cursor = end;
    }
    if cursor < run_range.end {
        out.push(gpui::TextRun { len: run_range.end - cursor, ..run });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clause(range: Range<usize>, thick: bool, line: ImeLine) -> ImeClause {
        ImeClause { range, thick, line }
    }

    fn seg(range: Range<usize>, thick: bool, line: ImeLine) -> ImeSegment {
        ImeSegment { range_start: range.start, range_end: range.end, thick, line }
    }

    #[test]
    fn no_clauses_is_one_thin_solid_line() {
        assert_eq!(ime_segments("かな", &[]), vec![seg(0..6, false, ImeLine::Solid)]);
    }

    #[test]
    fn clauses_become_byte_ranges() {
        // "変換中" (3 bytes each, 1 UTF-16 unit each): converting the first clause, converted the second, typing the third.
        let clauses = [
            clause(0..1, true, ImeLine::Solid),
            clause(1..2, false, ImeLine::Solid),
            clause(2..3, false, ImeLine::Dotted),
        ];
        assert_eq!(
            ime_segments("変換中", &clauses),
            vec![seg(0..3, true, ImeLine::Solid), seg(3..6, false, ImeLine::Solid), seg(6..9, false, ImeLine::Dotted)]
        );
    }

    #[test]
    fn surrogate_pairs_count_two_units() {
        // U+20BB7 is 2 UTF-16 units and 4 bytes.
        let clauses = [clause(0..2, true, ImeLine::Solid), clause(2..3, false, ImeLine::Wavy)];
        assert_eq!(ime_segments("\u{20BB7}a", &clauses), vec![seg(0..4, true, ImeLine::Solid), seg(4..5, false, ImeLine::Wavy)]);
    }

    #[test]
    fn uncovered_characters_get_the_thin_solid_line_and_neighbours_merge() {
        let clauses = [clause(1..2, true, ImeLine::Dotted)];
        assert_eq!(
            ime_segments("abc", &clauses),
            vec![seg(0..1, false, ImeLine::Solid), seg(1..2, true, ImeLine::Dotted), seg(2..3, false, ImeLine::Solid)]
        );
        let clauses = [clause(0..1, false, ImeLine::Solid), clause(1..3, false, ImeLine::Solid)];
        assert_eq!(ime_segments("abc", &clauses), vec![seg(0..3, false, ImeLine::Solid)]);
    }

    #[test]
    fn clause_ranges_past_the_text_are_clipped() {
        let clauses = [clause(0..9, false, ImeLine::Dotted)];
        assert_eq!(ime_segments("ab", &clauses), vec![seg(0..2, false, ImeLine::Dotted)]);
    }

    #[test]
    fn underline_style_by_line_and_thickness() {
        let c = gpui::black();
        let thin = seg(0..1, false, ImeLine::Solid).text_underline(c).unwrap();
        assert_eq!((thin.thickness, thin.wavy), (px(1.), false));
        let thick = seg(0..1, true, ImeLine::Solid).text_underline(c).unwrap();
        assert_eq!((thick.thickness, thick.wavy), (px(2.), false));
        let wavy = seg(0..1, false, ImeLine::Wavy).text_underline(c).unwrap();
        assert_eq!((wavy.thickness, wavy.wavy), (px(1.), true));
        assert!(seg(0..1, false, ImeLine::Dotted).text_underline(c).is_none());
    }

    #[test]
    fn runs_are_cut_at_the_segments() {
        let run = gpui::TextStyle::default().to_run(10);
        let c = gpui::black();
        let segments = vec![
            (2..5, seg(2..5, true, ImeLine::Solid).text_underline(c)),
            (5..7, seg(5..7, false, ImeLine::Dotted).text_underline(c)),
        ];
        let out = split_run_for_ime_segments(run, 0..10, &segments);
        let lens: Vec<usize> = out.iter().map(|r| r.len).collect();
        assert_eq!(lens, vec![2, 3, 2, 3]);
        assert!(out[0].underline.is_none());
        assert_eq!(out[1].underline.unwrap().thickness, px(2.));
        assert!(out[2].underline.is_none());
        assert!(out[3].underline.is_none());
    }

    #[test]
    fn runs_outside_the_segments_are_untouched() {
        let run = gpui::TextStyle::default().to_run(4);
        let out = split_run_for_ime_segments(run, 10..14, &[(0..5, None), (20..25, None)]);
        assert_eq!(out.iter().map(|r| r.len).collect::<Vec<_>>(), vec![4]);
    }
}
