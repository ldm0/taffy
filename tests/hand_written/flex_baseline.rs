use super::test_tree::{TestNode, TestTree};
use taffy::prelude::*;
use taffy::{BaselineType, Direction, LayoutInput, Point, RunMode, WritingMode};

#[test]
fn empty_flex_item_baselines_match_chromium_across_logical_flows() {
    // These filled lines have the same geometry for first and last baseline;
    // the independent Chromium matrix was checked with both preferences.
    for alignment in [AlignItems::BASELINE, AlignItems::LAST_BASELINE] {
        assert_empty_flex_item_baselines(alignment);
    }
}

fn assert_empty_flex_item_baselines(alignment: AlignItems) {
    // Chromium 145 geometry: fixed 100x100 flex container, four empty items,
    // align-items:baseline, no font metrics or rounding-dependent text widths.
    // Literal expectations are deliberately independent of Taffy's axis mapping.
    let cases: serde_json::Value = serde_json::from_str(include_str!("flex_baseline_geometry.json")).unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 80);
    for case in cases.as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let parts: Vec<_> = id.split('/').collect();
        let mode = match parts[0] {
            "horizontal-tb" => WritingMode::HorizontalTb,
            "vertical-rl" => WritingMode::VerticalRl,
            "vertical-lr" => WritingMode::VerticalLr,
            "sideways-rl" => WritingMode::SidewaysRl,
            "sideways-lr" => WritingMode::SidewaysLr,
            _ => panic!("unknown writing mode: {id}"),
        };
        let mut root = TestNode::container(
            Display::Flex,
            Style {
                size: Size::from_lengths(100.0, 100.0),
                direction: if parts[1] == "rtl" { Direction::Rtl } else { Direction::Ltr },
                flex_direction: match parts[2] {
                    "row" => FlexDirection::Row,
                    "row-reverse" => FlexDirection::RowReverse,
                    "column" => FlexDirection::Column,
                    "column-reverse" => FlexDirection::ColumnReverse,
                    _ => panic!("unknown flex direction: {id}"),
                },
                flex_wrap: if parts[3] == "wrap-reverse" { FlexWrap::WrapReverse } else { FlexWrap::Wrap },
                align_items: Some(alignment),
                ..Style::default()
            },
            Rect::ZERO,
        );
        root.writing_mode = mode;
        root.children = vec![1, 2, 3, 4];
        let mut tree = TestTree { nodes: vec![root] };
        for (width, height) in [(50.0, 50.0), (30.0, 50.0), (40.0, 70.0), (50.0, 20.0)] {
            let mut child = TestNode::leaf(
                Style { size: Size::from_lengths(width, height), flex_shrink: 0.0, ..Style::default() },
                Size::ZERO,
            );
            child.writing_mode = mode;
            tree.nodes.push(child);
        }
        tree.compute(Size::MAX_CONTENT);
        for index in 1..=4 {
            let layout = tree.layout(index);
            let expected = &case["children"][index - 1];
            for (axis, actual) in
                [layout.location.x, layout.location.y, layout.size.width, layout.size.height].into_iter().enumerate()
            {
                assert_eq!(
                    actual as f64,
                    expected[axis].as_f64().unwrap(),
                    "{alignment:?}/{id} child {index} component {axis}"
                );
            }
        }
    }
}

fn baseline_leaf(mode: WritingMode, first: f32, last: f32) -> TestNode {
    let mut node = TestNode::leaf(
        Style { size: Size::from_lengths(40.0, 60.0), flex_shrink: 0.0, ..Style::default() },
        Size::ZERO,
    );
    node.writing_mode = mode;
    node.first_baselines =
        if mode.is_horizontal() { Point { x: None, y: Some(first) } } else { Point { x: Some(first), y: None } };
    node.last_baselines =
        if mode.is_horizontal() { Point { x: None, y: Some(last) } } else { Point { x: Some(last), y: None } };
    node
}

#[test]
fn flex_exports_distinct_first_and_last_baselines_on_its_block_axis() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        let mut root = TestNode::container(Display::Flex, Style::default(), Rect::ZERO);
        root.writing_mode = mode;
        let mut tree = TestTree::new(root, baseline_leaf(mode, 13.0, 29.0));
        let output = tree.compute_child_layout(
            NodeId::from(0_usize),
            LayoutInput { run_mode: RunMode::PerformLayout, ..LayoutInput::HIDDEN },
        );
        if mode.is_horizontal() {
            assert_eq!(output.first_baselines, Point { x: None, y: Some(13.0) }, "{mode:?}");
            assert_eq!(output.last_baselines, Point { x: None, y: Some(29.0) }, "{mode:?}");
        } else {
            assert_eq!(output.first_baselines, Point { x: Some(13.0), y: None }, "{mode:?}");
            assert_eq!(output.last_baselines, Point { x: Some(29.0), y: None }, "{mode:?}");
        }
    }
}

#[test]
fn baseline_synthesis_respects_embedding_text_orientation() {
    // Central baselines align the item centers; vertical alphabetic baselines
    // align their physical left border edges, independently of their widths.
    for (baseline_type, expected) in [(BaselineType::Central, 10.0), (BaselineType::Alphabetic, 0.0)] {
        let mut root = TestNode::container(
            Display::Flex,
            Style {
                size: Size::from_lengths(100.0, 150.0),
                align_items: Some(AlignItems::BASELINE),
                ..Style::default()
            },
            Rect::ZERO,
        );
        root.writing_mode = WritingMode::VerticalRl;
        root.baseline_type = Some(baseline_type);
        root.children = vec![1, 2];
        let mut tree = TestTree { nodes: vec![root] };
        for width in [40.0, 20.0] {
            let mut child =
                TestNode::leaf(Style { size: Size::from_lengths(width, 30.0), ..Style::default() }, Size::ZERO);
            child.writing_mode = WritingMode::VerticalRl;
            tree.nodes.push(child);
        }
        tree.compute(Size::MAX_CONTENT);
        assert_eq!(tree.layout(2).location.x - tree.layout(1).location.x, expected, "{baseline_type:?}");
    }
}

#[test]
fn mixed_writing_modes_export_their_own_first_and_last_sharing_groups() {
    let mut root = TestNode::container(
        Display::Flex,
        Style { size: Size::from_lengths(100.0, 200.0), align_items: Some(AlignItems::BASELINE), ..Style::default() },
        Rect::ZERO,
    );
    root.writing_mode = WritingMode::VerticalRl;
    root.children = vec![1, 2, 3, 4];
    // Put the minor group first in DOM order: selecting the first baseline
    // item instead of the major group would export the wrong first baseline.
    let mut tree = TestTree {
        nodes: vec![
            root,
            baseline_leaf(WritingMode::VerticalLr, 8.0, 30.0),
            baseline_leaf(WritingMode::VerticalRl, 13.0, 29.0),
            baseline_leaf(WritingMode::VerticalRl, 5.0, 15.0),
            baseline_leaf(WritingMode::VerticalLr, 6.0, 10.0),
        ],
    };
    for index in 1..=4 {
        tree.nodes[index].style.size.height = length(30.0);
    }
    tree.nodes[3].style.size.width = length(20.0);
    tree.nodes[4].style.size.width = length(20.0);
    let output = tree.compute_child_layout(
        NodeId::from(0_usize),
        LayoutInput { run_mode: RunMode::PerformLayout, ..LayoutInput::HIDDEN },
    );
    assert_eq!(output.first_baselines, Point { x: Some(73.0), y: None });
    assert_eq!(output.last_baselines, Point { x: Some(8.0), y: None });
    assert_eq!([1, 2, 3, 4].map(|index| tree.layout(index).location.x), [0.0, 60.0, 68.0, 2.0]);
}

#[test]
fn auto_cross_margins_do_not_participate_in_baseline_sharing_or_export() {
    let mut root = TestNode::container(
        Display::Flex,
        Style { size: Size::from_lengths(150.0, 100.0), align_items: Some(AlignItems::BASELINE), ..Style::default() },
        Rect::ZERO,
    );
    root.children = vec![1, 2, 3];
    let mut auto_item = baseline_leaf(WritingMode::HorizontalTb, 90.0, 90.0);
    auto_item.style.margin.top = auto();
    let mut first = baseline_leaf(WritingMode::HorizontalTb, 13.0, 29.0);
    first.style.inset.top = length(7.0);
    let mut tree =
        TestTree { nodes: vec![root, auto_item, first, baseline_leaf(WritingMode::HorizontalTb, 5.0, 29.0)] };
    let output = tree.compute_child_layout(
        NodeId::from(0_usize),
        LayoutInput { run_mode: RunMode::PerformLayout, ..LayoutInput::HIDDEN },
    );
    assert_eq!(tree.layout(1).location.y, 40.0);
    assert_eq!(tree.layout(2).location.y, 7.0);
    assert_eq!(tree.layout(3).location.y, 8.0);
    assert_eq!(output.first_baselines.y, Some(13.0));
    assert_eq!(output.last_baselines.y, Some(13.0));
}
