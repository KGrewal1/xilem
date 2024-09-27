// Copyright 2018 the Xilem Authors and the Druid Authors
// SPDX-License-Identifier: Apache-2.0

//! A type for laying out, drawing, and interacting with text.

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use accesskit::{NodeBuilder, NodeId, Role};
use parley::context::RangedBuilder;
use parley::fontique::{Style, Weight};
use parley::layout::{Alignment, Cursor};
use parley::style::{Brush as BrushTrait, FontFamily, FontStack, GenericFamily, StyleProperty};
use parley::{FontContext, Layout, LayoutContext};
use unicode_segmentation::UnicodeSegmentation;
use vello::kurbo::{Affine, Line, Point, Rect, Size};
use vello::peniko::{self, Color, Gradient};
use vello::Scene;

use crate::{AccessCtx, WidgetId};

/// A component for displaying text on screen.
///
/// This is a type intended to be used by other widgets that display text.
/// It allows for the text itself as well as font and other styling information
/// to be set and modified. It wraps an inner layout object, and handles
/// invalidating and rebuilding it as required.
///
/// This object is not valid until the [`rebuild_if_needed`] method has been
/// called. You should generally do this in your widget's [`layout`] method.
/// Additionally, you should call [`needs_rebuild_after_update`]
/// as part of your widget's [`update`] method; if this returns `true`, you will need
/// to call [`rebuild_if_needed`] again, generally by scheduling another [`layout`]
/// pass.
///
/// [`layout`]: trait.Widget.html#tymethod.layout
/// [`update`]: trait.Widget.html#tymethod.update
/// [`needs_rebuild_after_update`]: #method.needs_rebuild_after_update
/// [`rebuild_if_needed`]: #method.rebuild_if_needed
///
/// TODO: Update docs to mentionParley
#[derive(Clone)]
pub struct TextLayout<T> {
    text: T,
    // TODO: Find a way to let this use borrowed data
    scale: f32,

    brush: TextBrush,
    font: FontStack<'static>,
    text_size: f32,
    weight: Weight,
    style: Style,

    alignment: Alignment,
    max_advance: Option<f32>,

    links: Rc<[(Rect, usize)]>,

    needs_layout: bool,
    needs_line_breaks: bool,
    pub(crate) layout: Layout<TextBrush>,
    scratch_scene: Scene,

    pub(crate) access_ids_by_run_path: HashMap<(usize, usize), NodeId>,
    pub(crate) run_paths_by_access_id: HashMap<NodeId, (usize, usize)>,
    pub(crate) character_lengths_by_access_id: HashMap<NodeId, Box<[u8]>>,
}

/// Whether a section of text should be hinted.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum Hinting {
    #[default]
    Yes,
    No,
}

impl Hinting {
    /// Whether the
    pub fn should_hint(self) -> bool {
        match self {
            Hinting::Yes => true,
            Hinting::No => false,
        }
    }
}

/// A custom brush for `Parley`, enabling using Parley to pass-through
/// which glyphs are selected/highlighted
#[derive(Clone, Debug, PartialEq)]
pub enum TextBrush {
    Normal(peniko::Brush, Hinting),
    Highlight {
        text: peniko::Brush,
        fill: peniko::Brush,
        hinting: Hinting,
    },
}

impl TextBrush {
    pub fn set_hinting(&mut self, hinting: Hinting) {
        match self {
            TextBrush::Normal(_, should_hint) => *should_hint = hinting,
            TextBrush::Highlight {
                hinting: should_hint,
                ..
            } => *should_hint = hinting,
        }
    }
}

impl BrushTrait for TextBrush {}

impl From<peniko::Brush> for TextBrush {
    fn from(value: peniko::Brush) -> Self {
        Self::Normal(value, Hinting::default())
    }
}

impl From<Gradient> for TextBrush {
    fn from(value: Gradient) -> Self {
        Self::Normal(value.into(), Hinting::default())
    }
}

impl From<Color> for TextBrush {
    fn from(value: Color) -> Self {
        Self::Normal(value.into(), Hinting::default())
    }
}

// Parley requires their Brush implementations to implement Default
impl Default for TextBrush {
    fn default() -> Self {
        Self::Normal(Default::default(), Hinting::default())
    }
}

/// Metrics describing the layout text.
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutMetrics {
    /// The nominal size of the layout.
    pub size: Size,
    /// The distance from the nominal top of the layout to the first baseline.
    pub first_baseline: f32,
    /// The width of the layout, inclusive of trailing whitespace.
    pub trailing_whitespace_width: f32,
    //TODO: add inking_rect
}

impl<T> TextLayout<T> {
    /// Create a new `TextLayout` object.
    pub fn new(text: T, text_size: f32) -> Self {
        TextLayout {
            text,
            scale: 1.0,

            brush: crate::theme::TEXT_COLOR.into(),
            font: FontStack::Single(FontFamily::Generic(GenericFamily::SansSerif)),
            text_size,
            weight: Weight::NORMAL,
            style: Style::Normal,

            max_advance: None,
            alignment: Default::default(),

            links: Rc::new([]),

            needs_layout: true,
            needs_line_breaks: true,
            layout: Layout::new(),
            scratch_scene: Scene::new(),

            access_ids_by_run_path: HashMap::new(),
            run_paths_by_access_id: HashMap::new(),
            character_lengths_by_access_id: HashMap::new(),
        }
    }

    /// Mark that the inner layout needs to be updated.
    ///
    /// This should be used if your `T` has interior mutability
    pub fn invalidate(&mut self) {
        self.needs_layout = true;
        self.needs_line_breaks = true;
    }

    /// Set the scaling factor
    pub fn set_scale(&mut self, scale: f32) {
        if scale != self.scale {
            self.scale = scale;
            self.invalidate();
        }
    }

    /// Set the default brush used for the layout.
    ///
    /// This is the non-layout impacting styling (primarily colour)
    /// used when displaying the text
    #[doc(alias = "set_color")]
    pub fn set_brush(&mut self, brush: impl Into<TextBrush>) {
        let brush = brush.into();
        if brush != self.brush {
            self.brush = brush;
            self.invalidate();
        }
    }

    /// Set the default font stack.
    pub fn set_font(&mut self, font: FontStack<'static>) {
        if font != self.font {
            self.font = font;
            self.invalidate();
        }
    }

    /// Set the font size.
    #[doc(alias = "set_font_size")]
    pub fn set_text_size(&mut self, size: f32) {
        if size != self.text_size {
            self.text_size = size;
            self.invalidate();
        }
    }

    /// Set the font weight.
    pub fn set_weight(&mut self, weight: Weight) {
        if weight != self.weight {
            self.weight = weight;
            self.invalidate();
        }
    }

    /// Set the font style.
    pub fn set_style(&mut self, style: Style) {
        if style != self.style {
            self.style = style;
            self.invalidate();
        }
    }

    /// Set the [`Alignment`] for this layout.
    pub fn set_text_alignment(&mut self, alignment: Alignment) {
        if self.alignment != alignment {
            self.alignment = alignment;
            self.invalidate();
        }
    }

    /// Set the width at which to wrap words.
    ///
    /// You may pass `None` to disable word wrapping
    /// (the default behaviour).
    pub fn set_max_advance(&mut self, max_advance: Option<f32>) {
        let max_advance = max_advance.map(|it| it.max(0.0));
        if self.max_advance.is_some() != max_advance.is_some()
            || self
                .max_advance
                .zip(max_advance)
                // 1e-4 is an arbitrary small-enough value that we don't care to rewrap
                .map(|(old, new)| (old - new).abs() >= 1e-4)
                .unwrap_or(false)
        {
            self.max_advance = max_advance;
            self.needs_line_breaks = true;
        }
    }

    /// Returns `true` if this layout needs to be rebuilt.
    ///
    /// This happens (for instance) after style attributes are modified.
    ///
    /// This does not account for things like the text changing, handling that
    /// is the responsibility of the user.
    #[must_use = "Has no side effects"]
    pub fn needs_rebuild(&self) -> bool {
        self.needs_layout || self.needs_line_breaks
    }

    // TODO: What are the valid use cases for this, where we shouldn't use a run-specific check instead?
    // /// Returns `true` if this layout's text appears to be right-to-left.
    // ///
    // /// See [`piet::util::first_strong_rtl`] for more information.
    // ///
    // /// [`piet::util::first_strong_rtl`]: crate::piet::util::first_strong_rtl
    // pub fn text_is_rtl(&self) -> bool {
    //     self.text_is_rtl
    // }
}

impl<T: AsRef<str> + Eq> TextLayout<T> {
    #[track_caller]
    fn assert_rebuilt(&self, method: &str) {
        if self.needs_layout || self.needs_line_breaks {
            debug_panic!(
                "TextLayout::{method} called without rebuilding layout object. Text was '{}'",
                self.text.as_ref().chars().take(250).collect::<String>()
            );
        }
    }

    /// Set the text to display.
    pub fn set_text(&mut self, text: T) {
        if self.text != text {
            self.text = text;
            self.invalidate();
        }
    }

    /// Returns the string backing this layout, if it exists.
    pub fn text(&self) -> &T {
        &self.text
    }

    /// Returns the string backing this layout, if it exists.
    ///
    /// Invalidates the layout and so should only be used when definitely applying an edit
    pub fn text_mut(&mut self) -> &mut T {
        self.invalidate();
        &mut self.text
    }

    /// Returns the inner Parley [`Layout`] value.
    pub fn layout(&self) -> &Layout<TextBrush> {
        self.assert_rebuilt("layout");
        &self.layout
    }

    /// The size of the laid-out text, excluding any trailing whitespace.
    ///
    /// This is not meaningful until [`Self::rebuild`] has been called.
    pub fn size(&self) -> Size {
        self.assert_rebuilt("size");
        Size::new(self.layout.width().into(), self.layout.height().into())
    }

    /// The size of the laid-out text, including any trailing whitespace.
    ///
    /// This is not meaningful until [`Self::rebuild`] has been called.
    pub fn full_size(&self) -> Size {
        self.assert_rebuilt("full_size");
        Size::new(self.layout.full_width().into(), self.layout.height().into())
    }

    /// Return the text's [`LayoutMetrics`].
    ///
    /// This is not meaningful until [`Self::rebuild`] has been called.
    pub fn layout_metrics(&self) -> LayoutMetrics {
        self.assert_rebuilt("layout_metrics");

        let first_baseline = self.layout.get(0).unwrap().metrics().baseline;
        let size = Size::new(self.layout.width().into(), self.layout.height().into());
        LayoutMetrics {
            size,
            first_baseline,
            trailing_whitespace_width: self.layout.full_width(),
        }
    }

    /// For a given `Point` (relative to this object's origin), returns index
    /// into the underlying text of the nearest grapheme boundary.
    ///
    /// This is not meaningful until [`Self::rebuild`] has been called.
    pub fn cursor_for_point(&self, point: Point) -> Cursor {
        self.assert_rebuilt("text_position_for_point");

        // TODO: This is a mostly good first pass, but doesn't handle cursor positions in
        // grapheme clusters within a parley cluster.
        // We can also try
        Cursor::from_point(&self.layout, point.x as f32, point.y as f32)
    }

    /// Given the utf-8 position of a character boundary in the underlying text,
    /// return the `Point` (relative to this object's origin) representing the
    /// boundary of the containing grapheme.
    ///
    /// # Panics
    ///
    /// Panics if `text_pos` is not a character boundary.
    ///
    /// This is not meaningful until [`Self::rebuild`] has been called.
    pub fn cursor_for_text_position(&self, text_pos: usize) -> Cursor {
        self.assert_rebuilt("cursor_for_text_position");

        // TODO: As a reminder, `is_leading` is not very useful to us; we don't know this ahead of time
        // We're going to need to do quite a bit of remedial work on these
        // e.g. to handle a inside a ligature made of multiple (unicode) grapheme clusters
        // https://raphlinus.github.io/text/2020/10/26/text-layout.html#shaping-cluster
        // But we're choosing to defer this work
        // This also needs to handle affinity.
        Cursor::from_position(&self.layout, text_pos, true)
    }

    /// Given the utf-8 position of a character boundary in the underlying text,
    /// return the `Point` (relative to this object's origin) representing the
    /// boundary of the containing grapheme.
    ///
    /// # Panics
    ///
    /// Panics if `text_pos` is not a character boundary.
    ///
    /// This is not meaningful until [`Self::rebuild`] has been called.
    pub fn point_for_text_position(&self, text_pos: usize) -> Point {
        let cursor = self.cursor_for_text_position(text_pos);
        Point::new(
            cursor.advance as f64,
            (cursor.baseline + cursor.offset) as f64,
        )
    }

    // TODO: needed for text selection
    // /// Given a utf-8 range in the underlying text, return a `Vec` of `Rect`s
    // /// representing the nominal bounding boxes of the text in that range.
    // ///
    // /// # Panics
    // ///
    // /// Panics if the range start or end is not a character boundary.
    // pub fn rects_for_range(&self, range: Range<usize>) -> Vec<Rect> {
    //     self.layout.rects_for_range(range)
    // }

    /// Given the utf-8 position of a character boundary in the underlying text,
    /// return a `Line` suitable for drawing a vertical cursor at that boundary.
    ///
    /// This is not meaningful until [`Self::rebuild`] has been called.
    // TODO: This is too simplistic. See https://raphlinus.github.io/text/2020/10/26/text-layout.html#shaping-cluster
    // for example. This would break in a `fi` ligature
    pub fn cursor_line_for_text_position(&self, text_pos: usize) -> Line {
        let from_position = self.cursor_for_text_position(text_pos);

        let line = from_position.path.line(&self.layout).unwrap();
        let line_metrics = line.metrics();

        let baseline = line_metrics.baseline + line_metrics.descent;
        let p1 = (from_position.offset as f64, baseline as f64);
        let p2 = (
            from_position.offset as f64,
            (baseline - line_metrics.size()) as f64,
        );
        Line::new(p1, p2)
    }

    /// Rebuild the inner layout as needed.
    ///
    /// This `TextLayout` object manages a lower-level layout object that may
    /// need to be rebuilt in response to changes to the text or attributes
    /// like the font.
    ///
    /// This method should be called whenever any of these things may have changed.
    /// A simple way to ensure this is correct is to always call this method
    /// as part of your widget's [`layout`][crate::Widget::layout] method.
    pub fn rebuild(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
    ) {
        self.rebuild_with_attributes(font_ctx, layout_ctx, |builder| builder);
    }

    /// Rebuild the inner layout as needed, adding attributes to the underlying layout.
    ///
    /// See [`Self::rebuild`] for more information
    pub fn rebuild_with_attributes(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        attributes: impl for<'b> FnOnce(
            RangedBuilder<'b, TextBrush, &'b str>,
        ) -> RangedBuilder<'b, TextBrush, &'b str>,
    ) {
        if self.needs_layout {
            self.needs_layout = false;

            let mut builder = layout_ctx.ranged_builder(font_ctx, self.text.as_ref(), self.scale);
            builder.push_default(&StyleProperty::Brush(self.brush.clone()));
            builder.push_default(&StyleProperty::FontSize(self.text_size));
            builder.push_default(&StyleProperty::FontStack(self.font));
            builder.push_default(&StyleProperty::FontWeight(self.weight));
            builder.push_default(&StyleProperty::FontStyle(self.style));

            // Currently, this is used for:
            // - underlining IME suggestions
            // - applying a brush to selected text.
            let mut builder = attributes(builder);
            builder.build_into(&mut self.layout);

            self.needs_line_breaks = true;
        }
        if self.needs_line_breaks {
            self.needs_line_breaks = false;
            self.layout
                .break_all_lines(self.max_advance, self.alignment);

            // TODO:
            // self.links = text
            //     .links()
            // ...
        }
    }

    /// Draw the layout at the provided `Point`.
    ///
    /// The origin of the layout is the top-left corner.
    ///
    /// You must call [`Self::rebuild`] at some point before you first
    /// call this method.
    pub fn draw(&mut self, scene: &mut Scene, point: impl Into<Point>) {
        self.assert_rebuilt("draw");
        // TODO: This translation doesn't seem great
        let p: Point = point.into();
        crate::text_helpers::render_text(
            scene,
            &mut self.scratch_scene,
            Affine::translate((p.x, p.y)),
            &self.layout,
        );
    }

    pub fn accessibility(&mut self, ctx: &mut AccessCtx, parent_node: &mut NodeBuilder) {
        self.assert_rebuilt("accessibility");

        let text = self.text.as_ref();
        let mut ids = HashSet::<NodeId>::new();

        for (line_index, line) in self.layout.lines().enumerate() {
            let mut last_node: Option<(NodeId, NodeBuilder)> = None;

            for (run_index, run) in line.runs().enumerate() {
                let run_path = (line_index, run_index);
                let id = self
                    .access_ids_by_run_path
                    .get(&run_path)
                    .copied()
                    .unwrap_or_else(|| {
                        let id = NodeId::from(WidgetId::next());
                        self.access_ids_by_run_path.insert(run_path, id);
                        self.run_paths_by_access_id.insert(id, run_path);
                        id
                    });
                ids.insert(id);
                let mut node = NodeBuilder::new(Role::InlineTextBox);

                if let Some((last_id, mut last_node)) = last_node.take() {
                    last_node.set_next_on_line(id);
                    node.set_previous_on_line(last_id);
                    ctx.tree_update.nodes.push((last_id, last_node.build()));
                    parent_node.push_child(last_id);
                }

                // TODO: bounding rectangle and character position/width
                let run_text = &text[run.text_range()];
                node.set_value(run_text);

                let mut character_lengths = Vec::new();
                let mut word_lengths = Vec::new();
                let mut was_at_word_end = false;
                let mut last_word_start = 0;

                for grapheme in run_text.graphemes(true) {
                    let is_word_char = grapheme.chars().next().unwrap().is_alphanumeric();
                    if is_word_char && was_at_word_end {
                        word_lengths.push((character_lengths.len() - last_word_start) as _);
                        last_word_start = character_lengths.len();
                    }
                    was_at_word_end = !is_word_char;
                    character_lengths.push(grapheme.len() as _);
                }

                word_lengths.push((character_lengths.len() - last_word_start) as _);
                self.character_lengths_by_access_id
                    .insert(id, character_lengths.clone().into());
                node.set_character_lengths(character_lengths);
                node.set_word_lengths(word_lengths);

                last_node = Some((id, node));
            }

            if let Some((id, node)) = last_node {
                // TODO: trailing newline if not the last line?
                ctx.tree_update.nodes.push((id, node.build()));
                parent_node.push_child(id);
            }
        }

        let mut ids_to_remove = Vec::<NodeId>::new();
        let mut run_paths_to_remove = Vec::<(usize, usize)>::new();
        for (access_id, run_path) in self.run_paths_by_access_id.iter() {
            if !ids.contains(access_id) {
                ids_to_remove.push(*access_id);
                run_paths_to_remove.push(*run_path);
            }
        }
        for id in ids_to_remove {
            self.run_paths_by_access_id.remove(&id);
            self.character_lengths_by_access_id.remove(&id);
        }
        for run_path in run_paths_to_remove {
            self.access_ids_by_run_path.remove(&run_path);
        }
    }
}

impl<T: AsRef<str> + Eq> std::fmt::Debug for TextLayout<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("TextLayout")
            .field("text", &self.text.as_ref())
            .field("scale", &self.scale)
            .field("brush", &self.brush)
            .field("font", &self.font)
            .field("text_size", &self.text_size)
            .field("weight", &self.weight)
            .field("style", &self.style)
            .field("alignment", &self.alignment)
            .field("wrap_width", &self.max_advance)
            .field("outdated?", &self.needs_rebuild())
            .field("width", &self.layout.width())
            .field("height", &self.layout.height())
            .field("links", &self.links)
            .finish_non_exhaustive()
    }
}

impl<T: AsRef<str> + Eq + Default> Default for TextLayout<T> {
    fn default() -> Self {
        Self::new(Default::default(), crate::theme::TEXT_SIZE_NORMAL as f32)
    }
}
