use super::test_tree::{TestNode, TestTree};
use taffy::prelude::*;
use taffy::{Direction, LayoutInput, Point, RunMode};

fn leaf(index: usize, height: f32) -> TestNode {
    let mut node = TestNode::leaf(
        Style { size: Size::from_lengths(40.0, height), flex_shrink: 0.0, ..Style::default() },
        Size::ZERO,
    );
    if index < 2 {
        node.first_baselines.y = Some([10.0, 20.0][index]);
        node.last_baselines.y = Some([30.0, 50.0][index]);
    }
    node
}

#[test]
fn first_last_and_mixed_groups_match_chromium_for_flex_and_grid() {
    // Chromium-backed zero-font fixture: the children's two line baselines
    // are at 10/30 and 20/50, inside fixed 80/60 border-box heights.
    for display in [Display::Flex, Display::Grid] {
        for (name, alignment, auto_height, height, expected_y, expected_baselines) in [
            ("first-fixed", AlignItems::BASELINE, false, 150.0, vec![10.0, 0.0], (20.0, 20.0)),
            ("last-fixed", AlignItems::LAST_BASELINE, false, 150.0, vec![70.0, 50.0], (100.0, 100.0)),
            ("first-auto", AlignItems::BASELINE, true, 90.0, vec![10.0, 0.0], (20.0, 20.0)),
            ("last-auto", AlignItems::LAST_BASELINE, true, 100.0, vec![20.0, 0.0], (50.0, 50.0)),
            ("mixed-fixed", AlignItems::BASELINE, false, 150.0, vec![0.0, 90.0], (10.0, 140.0)),
            ("mixed-auto", AlignItems::BASELINE, true, 80.0, vec![0.0, 20.0], (10.0, 70.0)),
            ("first-nonparticipant", AlignItems::BASELINE, true, 120.0, vec![10.0, 0.0, 0.0], (20.0, 20.0)),
            ("last-nonparticipant", AlignItems::LAST_BASELINE, true, 120.0, vec![40.0, 20.0, 0.0], (70.0, 70.0)),
            ("auto-margin", AlignItems::BASELINE, false, 150.0, vec![70.0, 0.0], (20.0, 20.0)),
            ("asymmetric", AlignItems::LAST_BASELINE, false, 150.0, vec![63.0, 43.0], (93.0, 93.0)),
            ("negative", AlignItems::LAST_BASELINE, false, 150.0, vec![75.0, 55.0], (105.0, 105.0)),
            ("relative", AlignItems::LAST_BASELINE, false, 150.0, vec![80.0, 50.0], (100.0, 100.0)),
        ] {
            let mut root = TestNode::container(
                display,
                Style {
                    size: Size { width: length(120.0), height: if auto_height { auto() } else { length(height) } },
                    align_items: Some(alignment),
                    grid_template_columns: vec![length(40.0), length(40.0), length(40.0)],
                    ..Style::default()
                },
                Rect::ZERO,
            );
            root.children = (1..=expected_y.len()).collect();
            let mut tree = TestTree { nodes: vec![root] };
            for index in 0..expected_y.len() {
                let mut child = leaf(index, [80.0, 60.0, 120.0][index]);
                if index == 2 {
                    child.style.align_self = Some(AlignSelf::START);
                }
                if name.starts_with("mixed") && index == 1 {
                    child.style.align_self = Some(AlignSelf::LAST_BASELINE);
                }
                if name == "auto-margin" && index == 0 {
                    child.style.margin.top = auto();
                }
                if name == "asymmetric" {
                    child.style.margin.top = length([5.0, 11.0][index]);
                    child.style.margin.bottom = length([7.0, 13.0][index]);
                }
                if name == "negative" {
                    child.style.margin.top = length([-10.0, -4.0][index]);
                    child.style.margin.bottom = length([-5.0, -3.0][index]);
                }
                if name == "relative" && index == 0 {
                    child.style.inset.top = length(10.0);
                }
                tree.nodes.push(child);
            }
            let output = tree.compute_child_layout(
                NodeId::from(0_usize),
                LayoutInput { run_mode: RunMode::PerformLayout, ..LayoutInput::HIDDEN },
            );
            assert_eq!(output.size, Size { width: 120.0, height }, "{display:?}/{name}");
            assert_eq!(
                (output.first_baselines.y, output.last_baselines.y),
                (Some(expected_baselines.0), Some(expected_baselines.1)),
                "{display:?}/{name} export"
            );
            for (index, y) in expected_y.into_iter().enumerate() {
                let layout = tree.layout(index + 1);
                assert_eq!(layout.location, Point { x: index as f32 * 40.0, y }, "{display:?}/{name}/{index}");
                let expected_margin = match name {
                    "auto-margin" if index == 0 => (70.0, 0.0),
                    "asymmetric" => ([5.0, 11.0][index], [7.0, 13.0][index]),
                    "negative" => ([-10.0, -4.0][index], [-5.0, -3.0][index]),
                    _ => (0.0, 0.0),
                };
                assert_eq!(
                    (layout.margin.top, layout.margin.bottom),
                    expected_margin,
                    "{display:?}/{name}/{index}: baseline shims must not become author margins"
                );
            }
        }
    }
}

#[test]
fn grid_spanning_baselines_belong_to_their_own_first_or_last_track() {
    for (alignment, second_start, height, expected_y, baselines) in [
        (AlignItems::BASELINE, 1, 110.0, [10.0, 0.0], (20.0, 40.0)),
        (AlignItems::LAST_BASELINE, 2, 200.0, [100.0, 80.0], (110.0, 130.0)),
    ] {
        let mut root = TestNode::container(
            Display::Grid,
            Style {
                size: Size { width: length(120.0), height: auto() },
                align_items: Some(alignment),
                grid_template_columns: vec![length(40.0), length(40.0), length(40.0)],
                grid_template_rows: vec![length(80.0), auto()],
                ..Style::default()
            },
            Rect::ZERO,
        );
        root.children = vec![1, 2];
        let mut first = leaf(0, 100.0);
        first.style.grid_row = Line { start: line(1), end: line(3) };
        let mut second = leaf(1, 60.0);
        second.style.grid_row = Line { start: line(second_start), end: line(second_start + 1) };
        let mut tree = TestTree { nodes: vec![root, first, second] };
        let output = tree.compute_child_layout(
            NodeId::from(0_usize),
            LayoutInput { run_mode: RunMode::PerformLayout, ..LayoutInput::HIDDEN },
        );
        assert_eq!(output.size.height, height);
        assert_eq!((output.first_baselines.y, output.last_baselines.y), (Some(baselines.0), Some(baselines.1)));
        for (index, y) in expected_y.into_iter().enumerate() {
            assert_eq!(tree.layout(index + 1).location, Point { x: index as f32 * 40.0, y });
            assert_eq!(tree.layout(index + 1).margin, Rect::ZERO);
        }
    }
}

#[test]
fn grid_baselines_measure_wrapping_in_the_resolved_columns() {
    // At min-content width the flex child has three lines and height 60.
    // Its actual 60px column instead gives it two lines, height 50 and a
    // last baseline at 50. The second item has baselines at 20 and 40.
    for (alignment, height, positions, baseline) in
        [(AlignItems::BASELINE, 60.0, [10.0, 0.0], 20.0), (AlignItems::LAST_BASELINE, 70.0, [0.0, 10.0], 50.0)]
    {
        let mut root = TestNode::container(
            Display::Grid,
            Style {
                size: Size { width: length(120.0), height: auto() },
                align_items: Some(alignment),
                grid_template_columns: vec![length(60.0), length(60.0)],
                ..Style::default()
            },
            Rect::ZERO,
        );
        root.children = vec![1, 2];
        let mut wrapped = TestNode::container(
            Display::Flex,
            Style { flex_wrap: FlexWrap::Wrap, align_items: Some(AlignItems::FLEX_START), ..Style::default() },
            Rect::ZERO,
        );
        wrapped.children = vec![3, 4, 5];
        let mut sibling = leaf(1, 60.0);
        sibling.style.size.width = length(60.0);
        sibling.last_baselines.y = Some(40.0);
        let mut tree = TestTree { nodes: vec![root, wrapped, sibling] };
        tree.nodes.extend([10.0, 20.0, 30.0].map(|height| {
            TestNode::leaf(
                Style { size: Size::from_lengths(30.0, height), flex_shrink: 0.0, ..Style::default() },
                Size::ZERO,
            )
        }));
        let output = tree.compute_child_layout(
            NodeId::from(0_usize),
            LayoutInput { run_mode: RunMode::PerformLayout, ..LayoutInput::HIDDEN },
        );
        assert_eq!(output.size, Size { width: 120.0, height });
        assert_eq!(output.first_baselines.y, Some(baseline));
        assert_eq!(output.last_baselines.y, Some(baseline));
        assert_eq!(tree.layout(1).size, Size { width: 60.0, height: 50.0 });
        for (index, y) in positions.into_iter().enumerate() {
            assert_eq!(tree.layout(index + 1).location, Point { x: index as f32 * 60.0, y });
            assert_eq!(tree.layout(index + 1).margin, Rect::ZERO);
        }
    }
}

#[test]
fn grid_inline_baseline_groups_follow_the_container_direction() {
    for (direction, alignment, x) in [
        (Direction::Ltr, AlignItems::BASELINE, 0.0),
        (Direction::Ltr, AlignItems::LAST_BASELINE, 40.0),
        (Direction::Rtl, AlignItems::BASELINE, 40.0),
        (Direction::Rtl, AlignItems::LAST_BASELINE, 0.0),
    ] {
        let mut root = TestNode::container(
            Display::Grid,
            Style {
                size: Size::from_lengths(120.0, 120.0),
                direction,
                align_items: Some(AlignItems::START),
                justify_items: Some(alignment),
                grid_template_columns: vec![length(120.0)],
                grid_template_rows: vec![length(60.0), length(60.0)],
                ..Style::default()
            },
            Rect::ZERO,
        );
        root.children = vec![1, 2];
        let mut tree = TestTree { nodes: vec![root] };
        tree.nodes.extend([80.0, 60.0].map(|width| {
            TestNode::leaf(Style { size: Size::from_lengths(width, 40.0), direction, ..Style::default() }, Size::ZERO)
        }));
        tree.compute(Size::MAX_CONTENT);
        for index in 0..2 {
            assert_eq!(tree.layout(index + 1).location, Point { x, y: index as f32 * 60.0 });
            assert_eq!(tree.layout(index + 1).margin, Rect::ZERO);
        }
    }
}

#[test]
fn grid_synthesized_percentage_baselines_do_not_create_track_sizing_cycles() {
    for (row, height) in [
        (auto::<GridTemplateComponent<String>>(), auto()),
        (minmax(length(0.0), fr(1.0)), auto()),
        (minmax(length(0.0), fr(1.0)), length(80.0)),
    ] {
        for (alignment, y) in [(AlignItems::BASELINE, 0.0), (AlignItems::LAST_BASELINE, 40.0)] {
            let mut root = TestNode::container(
                Display::Grid,
                Style {
                    size: Size { width: length(120.0), height },
                    align_items: Some(alignment),
                    grid_template_columns: vec![length(40.0), length(40.0)],
                    grid_template_rows: vec![row.clone()],
                    ..Style::default()
                },
                Rect::ZERO,
            );
            root.children = vec![1, 2];
            let first = TestNode::leaf(
                Style { size: Size { width: length(40.0), height: percent(0.5) }, ..Style::default() },
                Size::ZERO,
            );
            let mut tree = TestTree { nodes: vec![root, first, leaf(1, 80.0)] };
            tree.compute(Size::MAX_CONTENT);
            assert_eq!(tree.layout(0).size, Size { width: 120.0, height: 80.0 });
            assert_eq!(tree.layout(1).size, Size { width: 40.0, height: 40.0 });
            assert_eq!(tree.layout(1).location, Point { x: 0.0, y });
            assert_eq!(tree.layout(2).location, Point { x: 40.0, y: 0.0 });
        }
    }
}
