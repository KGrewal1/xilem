// Copyright 2022 The Druid Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use glazier::kurbo::{Affine, Point, Size};
use parley::Layout;
use vello::{
    peniko::{Brush, Color},
    SceneBuilder, SceneFragment,
};

use crate::text::ParleyBrush;

use super::{
    contexts::LifeCycleCx, BoxConstraints, ChangeFlags, Event, EventCx, LayoutCx, LifeCycle,
    PaintCx, UpdateCx, Widget,
};

pub struct TextWidget {
    text: String,
    layout: Option<Layout<ParleyBrush>>,
    is_wrapped: bool,
}

impl TextWidget {
    pub fn new(text: String) -> TextWidget {
        TextWidget {
            text,
            is_wrapped: false,
            layout: None,
        }
    }

    pub fn set_text(&mut self, text: String) -> ChangeFlags {
        self.text = text;
        ChangeFlags::LAYOUT | ChangeFlags::PAINT
    }
}

impl Widget for TextWidget {
    fn event(&mut self, _cx: &mut EventCx, _event: &Event) {}

    fn lifecycle(&mut self, _cx: &mut LifeCycleCx, _event: &LifeCycle) {}

    fn update(&mut self, cx: &mut UpdateCx) {
        // All changes potentially require layout. Note: we could be finer
        // grained, maybe color changes wouldn't.
        cx.request_layout();
    }

    fn layout(&mut self, cx: &mut LayoutCx, _proposed_size: &BoxConstraints) -> Size {
        let mut lcx = parley::LayoutContext::new();
        let mut layout_builder = lcx.ranged_builder(cx.font_cx(), &self.text, 1.0);
        layout_builder.push_default(&parley::style::StyleProperty::Brush(ParleyBrush(
            Brush::Solid(Color::rgb8(255, 255, 255)),
        )));
        let mut layout = layout_builder.build();
        // Question for Chad: is this needed?
        layout.break_all_lines(None, parley::layout::Alignment::Start);
        self.layout = Some(layout);
        cx.widget_state.size
    }

    fn accessibility(&mut self, cx: &mut super::AccessCx) {}

    fn paint(&mut self, cx: &mut PaintCx, builder: &mut SceneBuilder) {
        if let Some(layout) = &self.layout {
            let transform = Affine::translate((40.0, 40.0));
            crate::text::render_text(builder, transform, &layout);
        }
    }
}
