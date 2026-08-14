//! Behavioural tests against the public `HtmlGauge` contract: layout and
//! paint coordinates, data bindings, script callbacks, error handling and
//! the tolerance for unknown input.

use html_display::{HtmlGauge, PaintCmd, SimFrame};

fn frame(numbers: &[(&str, f64)]) -> SimFrame {
    SimFrame {
        time: 0.0,
        numbers: numbers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), *v))
            .collect(),
        lamps: Vec::new(),
        buttons: [false; 8],
    }
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.1
}

fn approx_color(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 0.01)
}

fn find_text(cmds: &[PaintCmd], wanted: &str) -> Option<(f32, f32, f32, [f32; 4])> {
    cmds.iter().find_map(|c| match c {
        PaintCmd::Text {
            x,
            y,
            text,
            size,
            color,
        } if text == wanted => Some((*x, *y, *size, *color)),
        _ => None,
    })
}

fn find_filled_rect(cmds: &[PaintCmd], color: [f32; 4]) -> Option<(f32, f32, f32, f32)> {
    cmds.iter().find_map(|c| match c {
        PaintCmd::Rect {
            x,
            y,
            w,
            h,
            color: rc,
            filled: true,
        } if approx_color(*rc, color) => Some((*x, *y, *w, *h)),
        _ => None,
    })
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 128.0 / 255.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const YELLOW: [f32; 4] = [1.0, 1.0, 0.0, 1.0];

#[test]
fn flex_page_paints_background_boxes_and_centered_text() {
    let html = r#"
        <style>
          body { background-color: #001100; }
          .row { display: flex; flex-direction: row; width: 200px; height: 100px; }
          .a { width: 50px; height: 100px; background-color: red; }
          .b { flex-grow: 1; text-align: center; font-size: 10px; }
        </style>
        <body><div class="row"><div class="a"></div><div class="b">HI</div></div></body>
    "#;
    let mut gauge = HtmlGauge::new(html, 200.0, 100.0).expect("gauge loads");
    let cmds = gauge.tick(&frame(&[])).expect("first tick paints");

    let PaintCmd::Clear { color } = &cmds[0] else {
        panic!("first command must be Clear, got {:?}", cmds[0]);
    };
    assert!(
        approx_color(*color, [0.0, 17.0 / 255.0, 0.0, 1.0]),
        "clear color from body background, got {color:?}"
    );

    let (x, y, w, h) = find_filled_rect(&cmds, RED).expect("red box painted");
    assert!(
        approx(x, 0.0) && approx(y, 0.0) && approx(w, 50.0) && approx(h, 100.0),
        "left box at (0,0,50,100), got ({x},{y},{w},{h})"
    );

    // ".b" spans x 50..200; "HI" is 2 chars * 0.6 * 10px = 12px wide,
    // centered: x = 50 + (150 - 12) / 2 = 119.
    let (x, y, size, color) = find_text(&cmds, "HI").expect("text painted");
    assert!(approx(x, 119.0), "centered text x, got {x}");
    assert!(approx(y, 0.0), "text y, got {y}");
    assert!(approx(size, 10.0), "font size, got {size}");
    assert!(approx_color(color, WHITE), "default text color, got {color:?}");
}

#[test]
fn data_bind_formats_and_reports_only_real_changes() {
    let html = r#"<div data-bind="v_kmh" data-format="%.1f"></div>"#;
    let mut gauge = HtmlGauge::new(html, 100.0, 50.0).expect("gauge loads");

    let cmds = gauge.tick(&frame(&[("v_kmh", 12.34)])).expect("initial paint");
    assert!(find_text(&cmds, "12.3").is_some(), "bound text painted");

    assert!(
        gauge.tick(&frame(&[("v_kmh", 12.34)])).is_none(),
        "unchanged value must not repaint"
    );

    let cmds = gauge
        .tick(&frame(&[("v_kmh", 12.36)]))
        .expect("changed value repaints");
    assert!(find_text(&cmds, "12.4").is_some(), "updated text painted");
}

#[test]
fn on_frame_text_mutation_repaints_once_per_change() {
    let html = r#"
        <div id="t"></div>
        <script>
          var el = document.getElementById("t");
          onFrame(function () { el.textContent = String(sim.v_kmh); });
        </script>
    "#;
    let mut gauge = HtmlGauge::new(html, 100.0, 50.0).expect("gauge loads");

    let cmds = gauge.tick(&frame(&[("v_kmh", 1.0)])).expect("first paint");
    assert!(find_text(&cmds, "1").is_some());

    assert!(
        gauge.tick(&frame(&[("v_kmh", 1.0)])).is_none(),
        "same text set again must not repaint"
    );

    let cmds = gauge.tick(&frame(&[("v_kmh", 2.0)])).expect("change repaints");
    assert!(find_text(&cmds, "2").is_some());
}

#[test]
fn on_button_fires_on_edges_with_index_and_state() {
    let html = r#"
        <div id="out"></div>
        <script>
          var out = document.getElementById("out");
          var log = [];
          onButton(function (i, p) {
            log.push(i + (p ? "+" : "-"));
            out.textContent = log.join(" ");
          });
        </script>
    "#;
    let mut gauge = HtmlGauge::new(html, 200.0, 50.0).expect("gauge loads");

    let cmds = gauge.tick(&frame(&[])).expect("initial paint");
    assert!(
        find_text(&cmds, "3+").is_none(),
        "no edge on the first tick with idle buttons"
    );

    let mut pressed = frame(&[]);
    pressed.buttons[2] = true;
    let cmds = gauge.tick(&pressed).expect("press edge repaints");
    assert!(
        find_text(&cmds, "3+").is_some(),
        "softkey 3 press reported as (3, true)"
    );

    let cmds = gauge.tick(&frame(&[])).expect("release edge repaints");
    assert!(
        find_text(&cmds, "3+ 3-").is_some(),
        "release reported as (3, false)"
    );
    assert!(gauge.take_errors().is_empty(), "no script errors");
}

#[test]
fn throwing_handler_is_disabled_and_reported_once() {
    let html = r#"
        <div data-bind="v_kmh" data-format="%d"></div>
        <script>onFrame(function () { throw new Error("boom"); });</script>
    "#;
    let mut gauge = HtmlGauge::new(html, 100.0, 50.0).expect("gauge loads");

    let cmds = gauge.tick(&frame(&[("v_kmh", 5.0)])).expect("initial paint");
    assert!(find_text(&cmds, "5").is_some());
    let errors = gauge.take_errors();
    assert_eq!(errors.len(), 1, "error reported exactly once: {errors:?}");
    assert!(errors[0].contains("boom"), "message carried: {}", errors[0]);

    assert!(gauge.tick(&frame(&[("v_kmh", 5.0)])).is_none());
    assert!(
        gauge.take_errors().is_empty(),
        "disabled handler must not report again"
    );

    let cmds = gauge
        .tick(&frame(&[("v_kmh", 7.0)]))
        .expect("gauge keeps painting after the handler died");
    assert!(find_text(&cmds, "7").is_some());
}

#[test]
fn display_none_removes_visibility_hidden_keeps_the_gap() {
    let html = r#"
        <style>
          div { width: 50px; height: 20px; }
          .gone { display: none; background-color: red; }
          .ghost { visibility: hidden; background-color: blue; }
          .solid { background-color: green; }
        </style>
        <div class="gone"></div><div class="ghost"></div><div class="solid"></div>
    "#;
    let mut gauge = HtmlGauge::new(html, 100.0, 100.0).expect("gauge loads");
    let cmds = gauge.tick(&frame(&[])).expect("paints");

    assert!(
        find_filled_rect(&cmds, RED).is_none(),
        "display:none paints nothing"
    );
    assert!(
        find_filled_rect(&cmds, BLUE).is_none(),
        "visibility:hidden paints nothing"
    );
    let (x, y, w, h) = find_filled_rect(&cmds, GREEN).expect("visible box painted");
    assert!(
        approx(y, 20.0),
        "hidden box keeps its 20px gap, none-box does not: y = {y}"
    );
    assert!(approx(x, 0.0) && approx(w, 50.0) && approx(h, 20.0));
}

#[test]
fn unknown_tags_and_properties_are_ignored() {
    let html = r#"
        <style>
          @media screen { .y { color: blue } }
          bar|baz { color: red }
          .z { }
        </style>
        <foo style="colour: red; color: yellow; frob: 3">hi</foo>
        <widget zap="1"></widget>
    "#;
    let mut gauge = HtmlGauge::new(html, 100.0, 50.0).expect("unknown input still loads");
    let cmds = gauge.tick(&frame(&[])).expect("paints");
    let (_, _, _, color) = find_text(&cmds, "hi").expect("text of unknown tag painted");
    assert!(
        approx_color(color, YELLOW),
        "known declaration applied, unknown ones skipped: {color:?}"
    );
    assert!(gauge.take_errors().is_empty(), "no errors for unknown input");
}
