//! Trassierung: aus einer verrauschten Punktfolge echte Entwurfselemente machen.
//!
//! Die naive Variante (Krümmung glätten und je Abtastschritt ein Segment) ergibt eine
//! befahrbare, aber erfundene Krümmungsfolge. Hier wird stattdessen rekonstruiert, was
//! ein Trassierer entworfen hätte:
//!
//! 1. **Abschnitte trennen**: gerade Bereiche und Bögen anhand der geglätteten Krümmung.
//! 2. **Radius ausgleichen**: Kreisausgleich (Kåsa) über den ganzen Bogen — das Rauschen
//!    der Stützpunkte mittelt sich mit √n heraus, während eine lokale Differenz daran
//!    scheitert (Pfeilhöhe einer 50-m-Sehne bei R = 1000 m: 31 cm gegenüber Metern
//!    Punktrauschen).
//! 3. **Richtungsänderung erhalten**: die gemessene Gesamtdrehung je Bogen bleibt
//!    erhalten, damit die Trasse nicht vom Original wegläuft.
//! 4. **Übergangsbögen und Überhöhung rechnen**: was sich aus den Daten nicht messen
//!    lässt, kommt aus dem Regelwerk — Überhöhung aus Radius und Streckengeschwindigkeit,
//!    Rampenlänge daraus.
//!
//! Ergebnis ist eine Kette aus Gerade – Klothoide – Kreisbogen – Klothoide – Gerade,
//! also genau die Darstellung, die `track-model` ohnehin führt.

use super::fit::SamplePoint;
use glam::DVec2;
use track_model::Segment;

/// Regelwerk für die Überhöhung.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CantRules {
    /// Höchste zulässige Überhöhung [mm].
    pub max_cant: f64,
    /// Zugelassener Überhöhungsfehlbetrag [mm] — so viel weniger als der Ausgleichswert
    /// wird eingebaut, damit langsamere Züge die Kurve nicht „nach innen" fahren.
    pub deficiency: f64,
    /// Rundung der eingebauten Überhöhung [mm].
    pub round_to: f64,
    /// Rampenneigung 1:(faktor·v) — bei 160 km/h und 100 mm also 1:1600, sprich 160 m.
    pub ramp_factor: f64,
    /// Kürzeste Übergangsrampe [m].
    pub min_ramp: f64,
}

impl Default for CantRules {
    fn default() -> Self {
        Self {
            max_cant: 160.0,
            deficiency: 60.0,
            round_to: 5.0,
            ramp_factor: 10.0,
            min_ramp: 20.0,
        }
    }
}

impl CantRules {
    /// Ausgleichsüberhöhung [mm]: `ü = 11,8 · v²/R`.
    ///
    /// Herleitung: `ü = G·v²/(g·R)` mit Radaufstandsbreite `G = 1500 mm`;
    /// mit `v` in km/h wird der Vorfaktor `1500/(9,81·3,6²) = 11,8`.
    pub fn equilibrium(radius: f64, v_kmh: f64) -> f64 {
        if radius.abs() < 1.0 {
            return 0.0;
        }
        11.8 * v_kmh * v_kmh / radius.abs()
    }

    /// Tatsächlich einzubauende Überhöhung [mm].
    pub fn applied(&self, radius: f64, v_kmh: f64) -> f64 {
        let raw = (Self::equilibrium(radius, v_kmh) - self.deficiency).clamp(0.0, self.max_cant);
        (raw / self.round_to).round() * self.round_to
    }

    /// Länge der Überhöhungsrampe [m] — zugleich die Mindestlänge des Übergangsbogens.
    pub fn ramp_length(&self, cant_mm: f64, v_kmh: f64) -> f64 {
        (cant_mm / 1000.0 * self.ramp_factor * v_kmh).max(self.min_ramp)
    }

    /// Auf die Einbaustufe runden.
    pub fn round(&self, cant_mm: f64) -> f64 {
        (cant_mm.clamp(0.0, self.max_cant) / self.round_to).round() * self.round_to
    }
}

/// Einstellungen der Trassierung.
#[derive(Debug, Clone, PartialEq)]
pub struct AlignmentOptions {
    /// Abtastabstand der Stützpunkte [m].
    pub sample: f64,
    /// Fenster der Richtungsschätzung (Punkte je Seite) — bei 20 m Abstand entsprechen
    /// 5 Punkte einer Basislänge von 200 m, über die sich Punktrauschen herausmittelt.
    pub window: usize,
    /// Basislänge der Krümmungsbildung (Punkte je Seite). Groß = rauschfest,
    /// klein = scharfe Abschnittsgrenzen.
    pub curvature_span: usize,
    /// Glättungsfenster der Krümmung (Punkte je Seite).
    pub smoothing: usize,
    /// Ab diesem Radius gilt der Abschnitt als gerade [m].
    ///
    /// Fahrdynamisch wäre schon ab ~8 km alles gerade — geometrisch nicht: ein
    /// 15-km-Bogen läuft über zwei Kilometer um mehr als 100 m von der Geraden weg.
    /// Deshalb wird großzügig als Bogen eingestuft; ein 30-km-Bogen bekommt ohnehin
    /// keine Überhöhung.
    pub straight_radius: f64,
    /// Kürzestes eigenständiges Element [m].
    ///
    /// Wirkt zugleich als Rauschfilter: kürzere „Bögen" in verrauschten Quelldaten sind
    /// fast immer Digitalisierungsfehler und werden dem Nachbarabschnitt zugeschlagen.
    pub min_element: f64,
    /// Radien auf die Regelreihe runden.
    pub snap_radii: bool,
    /// Nur runden, wenn der Regelradius innerhalb dieser relativen Abweichung liegt.
    /// Sonst bleibt der gemessene Wert stehen — ein aufgezwungener Regelradius, der
    /// mehrere Prozent danebenliegt, verzieht die ganze Kurve.
    pub snap_tolerance: f64,
    /// Regelradien, auf die gerundet wird [m].
    pub preferred_radii: Vec<f64>,
    pub cant: CantRules,
}

impl Default for AlignmentOptions {
    fn default() -> Self {
        Self {
            sample: 20.0,
            window: 5,
            curvature_span: 2,
            smoothing: 2,
            straight_radius: 30_000.0,
            min_element: 120.0,
            snap_radii: true,
            snap_tolerance: 0.04,
            preferred_radii: preferred_radii(),
            cant: CantRules::default(),
        }
    }
}

/// Übliche Entwurfsradien: fein gestuft im engen Bereich, gröber bei großen Radien.
fn preferred_radii() -> Vec<f64> {
    let mut radii = vec![150.0, 180.0, 190.0, 200.0, 225.0, 250.0, 275.0];
    let mut r = 300.0;
    while r < 2000.0 {
        radii.push(r);
        r += 50.0;
    }
    while r < 5000.0 {
        radii.push(r);
        r += 250.0;
    }
    while r <= 25_000.0 {
        radii.push(r);
        r += 500.0;
    }
    radii
}

/// Art eines Entwurfselements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Straight,
    /// Übergangsbogen (Klothoide).
    Transition,
    Arc,
}

/// Ein Entwurfselement — das, was im Trassierungsplan stünde.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Element {
    pub kind: ElementKind,
    /// Beginn ab Streckenanfang [m].
    pub start_s: f64,
    pub length: f64,
    /// Radius [m], positiv = Linksbogen.
    pub radius: Option<f64>,
    /// Überhöhung am Elementende [mm].
    pub cant: f64,
    /// Zulässige Geschwindigkeit, mit der gerechnet wurde [km/h].
    pub speed: f64,
}

/// Ergebnis der Trassierung.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    pub segments: Vec<Segment>,
    /// Entwurfselemente — Grundlage für Editor und Streckenband.
    pub elements: Vec<Element>,
    /// Überhöhungsstufen `(s, mm)`.
    pub cant: Vec<(f64, f64)>,
    /// Neigungsstufen `(s, ‰)`.
    pub grade: Vec<(f64, f64)>,
    /// Geschwindigkeitsstufen `(s, km/h)`.
    pub speed: Vec<(f64, f64)>,
    pub start_heading: f64,
    /// Größte Abweichung von der Punktfolge [m].
    pub max_deviation: f64,
}

impl Alignment {
    pub fn length(&self) -> f64 {
        self.segments.iter().map(|s| s.len).sum()
    }

    /// Anzahl Bögen — Kennzahl für den Importbericht.
    pub fn arcs(&self) -> usize {
        self.elements
            .iter()
            .filter(|e| e.kind == ElementKind::Arc)
            .count()
    }
}

/// Ein zusammenhängender Abschnitt gleicher Art in der Punktfolge.
#[derive(Debug, Clone, Copy)]
struct Run {
    start: usize,
    end: usize,
    curved: bool,
}

impl Run {
    fn len(&self, sample: f64) -> f64 {
        (self.end - self.start) as f64 * sample
    }
}

/// Trassiert die Punktfolge.
pub fn fit(points: &[SamplePoint], options: &AlignmentOptions) -> Alignment {
    assert!(points.len() >= 3, "mindestens drei Stützpunkte nötig");
    let h = options.sample;
    let headings = super::fit::headings(points, options.window);
    let curvature = super::fit::curvature(
        &headings,
        h,
        options.curvature_span,
        options.smoothing,
        options.window + options.curvature_span,
    );

    let runs = segment(&curvature, options);

    // Je Bogen: Radius ausgleichen und Drehwinkel messen.
    let mut plan: Vec<PlannedRun> = Vec::new();
    for (index, run) in runs.iter().enumerate() {
        if !run.curved {
            plan.push(PlannedRun {
                run: *run,
                radius: None,
                turn: 0.0,
                core: (run.start as f64 * h, run.end as f64 * h),
                cant: 0.0,
                ramp: 0.0,
                speed: run_speed(points, run),
            });
            continue;
        }
        let turn = measure_turn(&runs, index, &headings, &curvature, h);
        // Radius aus Länge und Drehwinkel — immer bestimmbar und in sich stimmig.
        let from_turn = if turn.abs() > 1e-9 {
            run.len(h) / turn.abs()
        } else {
            options.straight_radius
        };
        // Der Kreisausgleich ist genauer, solange er zum Abschnitt passt. Weicht die
        // Bogenlänge R·Δθ um mehr als ein Viertel von der gemessenen Abschnittslänge ab,
        // passen Radius und Drehwinkel nicht zusammen — dann würde die Strecke beim Bau
        // um diesen Betrag kürzer oder länger. In dem Fall gilt der stimmige Wert.
        // Ein Abschnitt besteht aus Bogen und zwei Übergangsbögen, seine Länge ist also
        // größer als R·Δθ — aber nicht beliebig: liegt der Bogen außerhalb dieses
        // Rahmens, passen Radius und Drehwinkel nicht zusammen (etwa bei stetig
        // zunehmender Krümmung), und der in sich stimmige Wert gewinnt.
        let measured = match fit_radius(points, run) {
            Some(fitted)
                if (0.4 * run.len(h)..1.25 * run.len(h)).contains(&(fitted * turn.abs())) =>
            {
                fitted
            }
            _ => from_turn,
        };
        let radius = if options.snap_radii {
            snap(measured, &options.preferred_radii, options.snap_tolerance)
        } else {
            measured
        };
        let speed = run_speed(points, run);

        // Überhöhung und Übergangslänge kommen aus dem Regelwerk, nicht aus den Daten:
        // die Länge eines Übergangsbogens lässt sich aus verrauschten Stützpunkten nicht
        // zurückgewinnen (die Abschnittsgrenze ist um über hundert Meter unsicher), die
        // Regelrampe zur eingebauten Überhöhung dagegen ist eindeutig bestimmt.
        // Aus den Daten stammen Lage, Radius und Drehwinkel des Bogens.
        let cant = options.cant.applied(radius, speed);
        let ramp = options.cant.ramp_length(cant, speed).min(run.len(h) * 0.45);
        plan.push(PlannedRun {
            run: *run,
            radius: Some(radius * turn.signum()),
            turn,
            core: arc_core(&curvature, run, 1.0 / radius, h),
            cant,
            ramp,
            speed,
        });
    }

    build(&plan, points, &headings, options)
}

/// Ein Abschnitt mit den bereits bestimmten Entwurfsgrößen.
#[derive(Debug, Clone, Copy)]
struct PlannedRun {
    run: Run,
    /// Vorzeichenbehafteter Radius [m]; `None` = Gerade.
    radius: Option<f64>,
    /// Gemessene Richtungsänderung [rad].
    turn: f64,
    /// Anfang und Ende des Bogenkerns als Bogenlänge ab Streckenanfang [m] —
    /// dort erreicht die Krümmung mindestens die Hälfte der Bogenkrümmung.
    core: (f64, f64),
    cant: f64,
    ramp: f64,
    speed: f64,
}

/// Kernbereich eines Bogens: dort erreicht die Krümmung mindestens die Hälfte der
/// Bogenkrümmung.
///
/// Seine Grenzen liegen — anders als die Abschnittsgrenzen — in der Mitte der jeweiligen
/// Übergangsbögen und sind gegen die Verschmierung der Schätzung unempfindlich, weil diese
/// symmetrisch wirkt. Sie sind damit der belastbarste Anhaltspunkt für die Lage des Bogens.
fn arc_core(curvature: &[f64], run: &Run, k_arc: f64, step: f64) -> (f64, f64) {
    let threshold = k_arc.abs() * 0.5;
    let core: Vec<usize> = (run.start..=run.end.min(curvature.len() - 1))
        .filter(|i| curvature[*i].abs() >= threshold)
        .collect();
    match (core.first(), core.last()) {
        (Some(a), Some(b)) => (*a as f64 * step, *b as f64 * step),
        _ => (run.start as f64 * step, run.end as f64 * step),
    }
}

/// Richtungsänderung eines Bogens [rad].
///
/// Gemessen wird zwischen den **Mitten der benachbarten Geraden**, nicht an den
/// Abschnittsgrenzen: dort steckt im Schätzfenster schon Krümmung, was den Winkel
/// systematisch zu klein macht — und ein um ein Prozent zu kleiner Drehwinkel schiebt
/// die Trasse hinter der Kurve um Meter zur Seite.
fn measure_turn(runs: &[Run], index: usize, headings: &[f64], curvature: &[f64], step: f64) -> f64 {
    let mid = |run: &Run| (run.start + run.end) / 2;
    let previous = index
        .checked_sub(1)
        .and_then(|i| runs.get(i))
        .filter(|r| !r.curved);
    let next = runs.get(index + 1).filter(|r| !r.curved);

    if let (Some(previous), Some(next)) = (previous, next) {
        return headings[mid(next)] - headings[mid(previous)];
    }
    // Am Datenrand fehlt eine der beiden Geraden. Die Richtung am äußersten Stützpunkt
    // taugt dort nicht — ihr Schätzfenster liegt nach innen verschoben und unterschätzt
    // die Drehung. Stattdessen wird die Krümmung über den Abschnitt aufintegriert.
    let run = &runs[index];
    curvature[run.start..=run.end.min(curvature.len() - 1)]
        .iter()
        .sum::<f64>()
        * step
}

/// Punktfolge in gerade und gekrümmte Abschnitte zerlegen.
///
/// Klassifiziert wird je Stützpunkt (Rechtsbogen / gerade / Linksbogen); anschließend
/// werden zu kurze Läufe von ihren Nachbarn verschluckt. Ohne diesen Schritt zerfällt
/// ein Bogen bei verrauschten Daten in Dutzende Schnipsel, und der Kreisausgleich
/// bekommt zu wenige Punkte.
fn segment(curvature: &[f64], options: &AlignmentOptions) -> Vec<Run> {
    let threshold = 1.0 / options.straight_radius;
    let min_points = (options.min_element / options.sample).ceil().max(2.0) as usize;

    let mut class: Vec<i8> = curvature
        .iter()
        .map(|k| {
            if k.abs() <= threshold {
                0
            } else {
                k.signum() as i8
            }
        })
        .collect();
    // Mehrere Durchgänge: nach dem Verschlucken können neue kurze Läufe entstehen.
    for _ in 0..4 {
        if !despeckle(&mut class, min_points) {
            break;
        }
    }

    let mut runs = Vec::new();
    let mut start = 0usize;
    for i in 1..class.len() {
        if class[i] != class[start] {
            runs.push(Run {
                start,
                end: i,
                curved: class[start] != 0,
            });
            start = i;
        }
    }
    runs.push(Run {
        start,
        end: class.len() - 1,
        curved: class[start] != 0,
    });
    runs
}

/// Läufe unterhalb der Mindestlänge dem längeren Nachbarn zuschlagen.
/// Gibt zurück, ob etwas geändert wurde.
fn despeckle(class: &mut [i8], min_points: usize) -> bool {
    let n = class.len();
    let mut bounds: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for i in 1..n {
        if class[i] != class[start] {
            bounds.push((start, i));
            start = i;
        }
    }
    bounds.push((start, n));

    let mut changed = false;
    for (index, &(from, to)) in bounds.iter().enumerate() {
        if to - from >= min_points {
            continue;
        }
        let previous = index.checked_sub(1).map(|i| bounds[i]);
        let next = bounds.get(index + 1).copied();
        let winner = match (previous, next) {
            (Some(p), Some(nx)) => {
                if p.1 - p.0 >= nx.1 - nx.0 {
                    class[p.0]
                } else {
                    class[nx.0]
                }
            }
            (Some(p), None) => class[p.0],
            (None, Some(nx)) => class[nx.0],
            (None, None) => continue,
        };
        if winner != class[from] {
            for c in &mut class[from..to] {
                *c = winner;
            }
            changed = true;
        }
    }
    changed
}

/// Zulässige Geschwindigkeit eines Abschnitts (die kleinste darin).
fn run_speed(points: &[SamplePoint], run: &Run) -> f64 {
    points[run.start..=run.end.min(points.len() - 1)]
        .iter()
        .map(|p| p.speed)
        .fold(f64::INFINITY, f64::min)
}

/// Kreisausgleich nach Kåsa über den Kern des Bogens.
///
/// Die äußeren 20 % bleiben außen vor — dort liegen die Übergangsbögen, deren Krümmung
/// noch nicht dem Kreis entspricht.
fn fit_radius(points: &[SamplePoint], run: &Run) -> Option<f64> {
    let count = run.end - run.start;
    if count < 4 {
        return None;
    }
    let margin = count / 5;
    let slice = &points[run.start + margin..=(run.end - margin).min(points.len() - 1)];
    if slice.len() < 4 {
        return None;
    }

    // Ausgleich von x² + y² + D·x + E·y + F = 0 im Schwerpunktsystem.
    let mean = slice.iter().fold(DVec2::ZERO, |a, p| a + p.pos) / slice.len() as f64;
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    let (mut sxz, mut syz) = (0.0, 0.0);
    for p in slice {
        let d = p.pos - mean;
        let z = d.x * d.x + d.y * d.y;
        sxx += d.x * d.x;
        syy += d.y * d.y;
        sxy += d.x * d.y;
        sxz += d.x * z;
        syz += d.y * z;
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() < 1e-9 {
        return None;
    }
    let cx = 0.5 * (sxz * syy - syz * sxy) / det;
    let cy = 0.5 * (syz * sxx - sxz * sxy) / det;
    let radius = (cx * cx
        + cy * cy
        + slice
            .iter()
            .map(|p| (p.pos - mean).length_squared())
            .sum::<f64>()
            / slice.len() as f64)
        .sqrt();
    radius.is_finite().then_some(radius)
}

/// Nächstgelegener Regelradius.
fn snap(radius: f64, preferred: &[f64], tolerance: f64) -> f64 {
    preferred
        .iter()
        .copied()
        .min_by(|a, b| (a - radius).abs().total_cmp(&(b - radius).abs()))
        .filter(|nearest| (nearest - radius).abs() <= radius * tolerance)
        .unwrap_or(radius)
}

/// Baut aus den geplanten Abschnitten die Segmentkette samt Überhöhungsband.
fn build(
    plan: &[PlannedRun],
    points: &[SamplePoint],
    headings: &[f64],
    options: &AlignmentOptions,
) -> Alignment {
    let h = options.sample;
    let mut segments: Vec<Segment> = Vec::new();
    let mut elements: Vec<Element> = Vec::new();
    let mut cant_steps: Vec<(f64, f64)> = vec![(0.0, 0.0)];
    let mut s = 0.0;

    // Jeder Bogen wird um seine Mitte gelegt und behält seinen Drehwinkel
    // (L_bogen = R·Δθ − L_rampe); die Geraden füllen die Lücken dazwischen. Die
    // Bogenmitte ist die einzige Größe, die sich aus verrauschten Daten zuverlässig
    // bestimmen lässt — Anfang und Ende eines Bogens nicht.
    let total = plan.last().map_or(0.0, |p| p.run.end as f64 * h);
    let mut lengths: Vec<f64> = vec![0.0; plan.len()];
    let mut cursor = 0.0;
    for (i, planned) in plan.iter().enumerate() {
        let Some(radius) = planned.radius else {
            continue;
        };
        let arc =
            (radius.abs() * planned.turn.abs() - planned.ramp).max(options.min_element * 0.25);
        let built = 2.0 * planned.ramp + arc;

        // Verankert wird am Bogenkern, und zwar an der Seite, an der eine Gerade
        // anschließt: dort ist die Lage am besten bestimmt (der Kernbeginn liegt in der
        // Mitte des Übergangsbogens, also eine halbe Rampe hinter dessen Anfang).
        // Endet der Bogen am Datenrand, wird die andere Seite verankert.
        let has_previous = i > 0 && plan[i - 1].radius.is_none();
        let has_next = i + 1 < plan.len() && plan[i + 1].radius.is_none();
        let start = match (has_previous, has_next) {
            (true, _) => planned.core.0 - planned.ramp / 2.0,
            (false, true) => planned.core.1 + planned.ramp / 2.0 - built,
            (false, false) => (planned.core.0 + planned.core.1) / 2.0 - built / 2.0,
        }
        .max(cursor);

        if i > 0 {
            lengths[i - 1] = start - cursor;
        }
        lengths[i] = built;
        cursor = start + built;
    }
    // Der Rest hinter dem letzten Bogen gehört zur letzten Geraden. Gibt es keine
    // (die Daten enden im Bogen), bleibt die Kette entsprechend kürzer — Länge zu
    // erfinden wäre schlimmer als sie zu verlieren.
    if let Some(last) = lengths.len().checked_sub(1)
        && plan[last].radius.is_none()
    {
        lengths[last] = (total - cursor).max(0.0);
    }

    for (index, planned) in plan.iter().enumerate() {
        let run_len = lengths[index].max(0.0);
        match planned.radius {
            None => {
                // Die Geraden behalten ihre gemessene Länge; die Übergangsbögen liegen
                // vollständig im jeweiligen Bogenabschnitt (siehe L_rampe oben).
                let length = run_len.max(options.min_element * 0.25);
                push_element(
                    &mut segments,
                    &mut elements,
                    &mut s,
                    ElementKind::Straight,
                    Segment::straight(length),
                    None,
                    0.0,
                    planned.speed,
                );
            }
            Some(radius) => {
                let k = 1.0 / radius;
                // Abschnittslänge und Drehwinkel bleiben erhalten:
                // L_abschnitt = 2·L_rampe + L_bogen und Δθ = (L_bogen + L_rampe)/R.
                let ramp = planned.ramp.min(run_len * 0.45);
                let arc_len = (run_len - 2.0 * ramp).max(options.min_element * 0.25);

                push_element(
                    &mut segments,
                    &mut elements,
                    &mut s,
                    ElementKind::Transition,
                    Segment::transition(ramp, 0.0, k),
                    Some(radius),
                    planned.cant,
                    planned.speed,
                );
                ramp_cant(&mut cant_steps, s - ramp, ramp, 0.0, planned.cant);

                push_element(
                    &mut segments,
                    &mut elements,
                    &mut s,
                    ElementKind::Arc,
                    Segment::arc(arc_len, radius),
                    Some(radius),
                    planned.cant,
                    planned.speed,
                );

                push_element(
                    &mut segments,
                    &mut elements,
                    &mut s,
                    ElementKind::Transition,
                    Segment::transition(ramp, k, 0.0),
                    Some(radius),
                    0.0,
                    planned.speed,
                );
                ramp_cant(&mut cant_steps, s - ramp, ramp, planned.cant, 0.0);
            }
        }
    }

    let start_heading = headings[0];
    let max_deviation = super::fit::deviation(&segments, start_heading, points);

    Alignment {
        segments,
        elements,
        cant: cant_steps,
        grade: super::fit::grade_profile(points, h),
        speed: super::fit::speed_profile(points, h),
        start_heading,
        max_deviation,
    }
}

#[allow(clippy::too_many_arguments)]
fn push_element(
    segments: &mut Vec<Segment>,
    elements: &mut Vec<Element>,
    s: &mut f64,
    kind: ElementKind,
    segment: Segment,
    radius: Option<f64>,
    cant: f64,
    speed: f64,
) {
    if segment.len <= 1e-6 {
        return;
    }
    elements.push(Element {
        kind,
        start_s: *s,
        length: segment.len,
        radius,
        cant,
        speed,
    });
    *s += segment.len;
    segments.push(segment);
}

/// Überhöhungsrampe als Stufen — `StepProfile` kennt keine Interpolation.
///
/// ponytail: 10-m-Stufen statt eines linearen Profils. Der Sprung je Stufe liegt bei
/// wenigen Millimetern und ist im Wanken nicht spürbar; wenn `StepProfile` einmal
/// interpolieren kann, entfällt das hier.
fn ramp_cant(steps: &mut Vec<(f64, f64)>, start: f64, length: f64, from: f64, to: f64) {
    if length <= 0.0 {
        return;
    }
    let count = (length / 10.0).ceil().max(1.0) as usize;
    for i in 0..=count {
        let t = i as f64 / count as f64;
        steps.push((start + length * t, from + (to - from) * t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Erzeugt eine Trasse aus Entwurfselementen und tastet sie ab —
    /// das Gegenstück zu dem, was der Fitter rekonstruieren soll.
    fn design_track(radius: f64, transition: f64, arc: f64, noise: f64) -> Vec<SamplePoint> {
        let step = 20.0;
        let mut pts = Vec::new();
        let mut pos = DVec2::ZERO;
        let mut heading = 0.0f64;
        let mut seed = 12345u64;
        let mut rand = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) - 1.0
        };

        let plan: Vec<(f64, f64, f64)> = vec![
            // (Länge, k_start, k_ende)
            (600.0, 0.0, 0.0),
            (transition, 0.0, 1.0 / radius),
            (arc, 1.0 / radius, 1.0 / radius),
            (transition, 1.0 / radius, 0.0),
            (600.0, 0.0, 0.0),
        ];
        for (len, k0, k1) in plan {
            let steps = (len / step).round() as usize;
            for i in 0..steps {
                let t = i as f64 / steps as f64;
                let k = k0 + (k1 - k0) * t;
                heading += k * step;
                pos += DVec2::new(heading.cos(), heading.sin()) * step;
                pts.push(SamplePoint {
                    pos: pos + DVec2::new(rand(), rand()) * noise,
                    height: 0.0,
                    speed: 160.0,
                });
            }
        }
        pts
    }

    #[test]
    fn ueberhoehung_folgt_der_formel() {
        let rules = CantRules::default();
        // 160 km/h in R = 2000 m: Ausgleichsüberhöhung 11,8·160²/2000 = 151 mm.
        let eq = CantRules::equilibrium(2000.0, 160.0);
        assert!((eq - 151.0).abs() < 1.0, "{eq}");
        // Eingebaut wird sie abzüglich des zugelassenen Fehlbetrags, auf 5 mm gerundet.
        assert_eq!(rules.applied(2000.0, 160.0), 90.0);
        // Enge Kurven laufen in die Obergrenze.
        assert_eq!(rules.applied(300.0, 100.0), rules.max_cant);
        // Auf der Geraden keine Überhöhung.
        assert_eq!(rules.applied(50_000.0, 160.0), 0.0);
        // Rampe: 90 mm bei 160 km/h → 1:1600, also 144 m.
        assert!((rules.ramp_length(90.0, 160.0) - 144.0).abs() < 1.0);
    }

    #[test]
    fn entwurfselemente_werden_zurueckgewonnen() {
        // Regelkonforme Quelle: die Übergangslänge entspricht der Rampe, die zur
        // Überhöhung dieses Bogens gehört (R = 1200 m bei 160 km/h → 160 mm → 256 m).
        let rules = CantRules::default();
        let transition = rules.ramp_length(rules.applied(1200.0, 160.0), 160.0);
        let points = design_track(1200.0, transition, 400.0, 0.0);
        let alignment = fit(&points, &AlignmentOptions::default());

        // Erwartet: Gerade – Übergang – Bogen – Übergang – Gerade.
        let kinds: Vec<ElementKind> = alignment.elements.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ElementKind::Straight,
                ElementKind::Transition,
                ElementKind::Arc,
                ElementKind::Transition,
                ElementKind::Straight,
            ],
            "{:?}",
            alignment.elements
        );
        assert_eq!(alignment.arcs(), 1);

        // Radius auf den Regelwert getroffen.
        let arc = alignment
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Arc)
            .unwrap();
        assert_eq!(arc.radius.unwrap().abs(), 1200.0);
        // Ein paar Meter Rest bleiben — dieselbe Größenordnung, in der auch die
        // Quelldaten selbst liegen (OSM aus Luftbildern: ±2…5 m). Genauer zu werden
        // hätte hier keinen Informationswert mehr.
        assert!(
            alignment.max_deviation < 6.0,
            "Rekonstruktionsfehler {:.1} m",
            alignment.max_deviation
        );

        // Die Elementlängen treffen den Entwurf.
        let built_transition = alignment.elements[1].length;
        assert!(
            (built_transition - transition).abs() < 30.0,
            "Übergangsbogen {built_transition:.0} m statt {transition:.0} m"
        );
    }

    #[test]
    fn abweichende_uebergangsboegen_bleiben_im_rahmen() {
        // Quelle mit einem kürzeren Übergangsbogen, als das Regelwerk zur Überhöhung
        // vorsieht. Rekonstruiert wird trotzdem regelkonform — die Trasse weicht dadurch
        // sichtbar, aber begrenzt ab. Aus verrauschten Punkten ist die tatsächliche
        // Übergangslänge nicht rückgewinnbar; die Abschnittsgrenze ist um mehr als
        // hundert Meter unsicher.
        let points = design_track(1200.0, 120.0, 400.0, 0.0);
        let alignment = fit(&points, &AlignmentOptions::default());
        assert_eq!(alignment.arcs(), 1);
        assert!(
            alignment.max_deviation < 12.0,
            "Abweichung {:.1} m",
            alignment.max_deviation
        );
    }

    #[test]
    fn radius_ueberlebt_verrauschte_punkte() {
        // ±2 m Rauschen — die Größenordnung von OSM aus Luftbildern.
        let points = design_track(800.0, 100.0, 500.0, 2.0);
        let alignment = fit(&points, &AlignmentOptions::default());
        let arc = alignment
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Arc)
            .expect("Bogen erkannt");
        let radius = arc.radius.unwrap().abs();
        assert!(
            (radius - 800.0).abs() <= 100.0,
            "Radius {radius} statt 800 m"
        );
    }

    #[test]
    fn ueberhoehung_landet_im_band() {
        let points = design_track(1200.0, 120.0, 400.0, 0.0);
        let alignment = fit(&points, &AlignmentOptions::default());

        let rules = CantRules::default();
        let max_cant = alignment.cant.iter().map(|(_, c)| *c).fold(0.0, f64::max);

        // Eingebaut wird der kleinere von zwei Werten: was das Regelwerk für Radius und
        // Geschwindigkeit vorsieht, und was die vorhandene Rampe hergibt.
        assert!(
            max_cant <= rules.applied(1200.0, 160.0),
            "über dem Regelwert: {max_cant}"
        );
        assert!(
            max_cant > 40.0,
            "Überhöhung sollte spürbar sein: {max_cant}"
        );
        let ramp = alignment
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Transition)
            .unwrap()
            .length;
        assert!(
            ramp >= rules.ramp_length(max_cant, 160.0) - 1.0,
            "Rampe {ramp:.0} m zu kurz für {max_cant} mm"
        );

        // Anfang und Ende liegen überhöhungsfrei.
        assert_eq!(alignment.cant[0], (0.0, 0.0));
        assert_eq!(alignment.cant.last().unwrap().1, 0.0);

        // Die Rampe steigt monoton bis zum Bogen und fällt danach wieder.
        let peak = alignment
            .cant
            .iter()
            .position(|(_, c)| *c >= max_cant)
            .unwrap();
        assert!(
            alignment.cant[..=peak].windows(2).all(|w| w[1].1 >= w[0].1),
            "Rampe steigt nicht monoton"
        );
    }

    #[test]
    fn gerade_bleibt_ein_einziges_element() {
        let points: Vec<SamplePoint> = (0..60)
            .map(|i| SamplePoint {
                pos: DVec2::new(i as f64 * 20.0, 0.0),
                height: 0.0,
                speed: 120.0,
            })
            .collect();
        let alignment = fit(&points, &AlignmentOptions::default());
        assert_eq!(alignment.elements.len(), 1);
        assert_eq!(alignment.elements[0].kind, ElementKind::Straight);
        assert!(alignment.cant.iter().all(|(_, c)| *c == 0.0));
    }

    #[test]
    fn gegenboegen_werden_getrennt() {
        // S-Kurve: erst links, dann rechts.
        let step = 20.0;
        let mut pos = DVec2::ZERO;
        let mut heading = 0.0f64;
        let mut pts = Vec::new();
        for i in 0..160 {
            let k = match i {
                0..=30 => 0.0,
                31..=70 => 1.0 / 1000.0,
                71..=110 => -1.0 / 1000.0,
                _ => 0.0,
            };
            heading += k * step;
            pos += DVec2::new(heading.cos(), heading.sin()) * step;
            pts.push(SamplePoint {
                pos,
                height: 0.0,
                speed: 120.0,
            });
        }
        let alignment = fit(&pts, &AlignmentOptions::default());
        assert_eq!(alignment.arcs(), 2, "{:?}", alignment.elements);
        let radii: Vec<f64> = alignment
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Arc)
            .map(|e| e.radius.unwrap())
            .collect();
        assert!(
            radii[0].signum() != radii[1].signum(),
            "Gegenbögen müssen entgegengesetzt sein: {radii:?}"
        );
    }

    #[test]
    fn ohne_rundung_bleibt_der_gemessene_radius() {
        let points = design_track(1150.0, 120.0, 400.0, 0.0);
        let options = AlignmentOptions {
            snap_radii: false,
            ..Default::default()
        };
        let alignment = fit(&points, &options);
        let arc = alignment
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Arc)
            .unwrap();
        let radius = arc.radius.unwrap().abs();
        assert!((radius - 1150.0).abs() < 60.0, "{radius}");
        assert_ne!(radius, 1200.0, "ohne Rundung kein Regelwert");
    }
}
