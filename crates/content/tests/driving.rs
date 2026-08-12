//! Abnahmetests der Fahrdynamik und Bremse (Plan Kap. 6, 7, 18) — headless.

use content::musterbahn;
use content::vehicles::{br101, de_pzb_lzb, freight_wagon, passenger_coach, vehicle};
use sim_core::Sim;
use sim_core::brakes::DriverBrakeValve;
use sim_core::safety::SafetySystems;
use sim_core::safety::de::TrainType;
use sim_core::train::{Train, Vehicle};
use track_model::{EdgeId, TrackPosition};

/// Baut einen Zug aus BR 101 + n Reisezugwagen am Streckenanfang.
fn passenger_train(sim: &mut Sim, coaches: usize) -> usize {
    let head = TrackPosition::new(EdgeId(0), 100.0, 1);
    let mut vehicles = vec![vehicle(br101(), head, de_pzb_lzb(TrainType::O))];
    for _ in 0..coaches {
        vehicles.push(vehicle(passenger_coach(), head, SafetySystems::None));
    }
    let train = Train::assemble(vehicles, head, &sim.net);
    sim.add_train(train)
}

fn new_sim() -> Sim {
    let line = musterbahn().compile().expect("Strecke übersetzbar");
    Sim::new(line.net, line.interlock, 1234)
}

/// Fahrzeuge betriebsbereit machen (Batterie, Bügel, Hauptschalter).
fn power_up(sim: &mut Sim, train: usize) {
    for v in &mut sim.trains[train].vehicles {
        if v.spec.traction.is_some() {
            v.traction.battery = true;
            v.traction.pantograph_command = true;
            v.traction.main_switch_command = true;
        }
    }
    // Stromabnehmer braucht ~5 s.
    for _ in 0..1600 {
        sim.step(Sim::DT);
    }
}

fn set_speed(sim: &mut Sim, train: usize, kmh: f64) {
    for v in &mut sim.trains[train].vehicles {
        v.v = kmh / 3.6;
    }
}

/// Sifa still halten (sonst greift sie nach 35 s ein).
fn hold_sifa(sim: &mut Sim, train: usize, pressed: bool) {
    sim.controls[train].sifa = pressed;
}

#[test]
fn auslaufversuch_folgt_davis_kurve() {
    let mut sim = new_sim();
    let t = passenger_train(&mut sim, 5);
    power_up(&mut sim, t);
    set_speed(&mut sim, t, 120.0);
    sim.controls[t].brake_valve = DriverBrakeValve::Release;

    // 60 s auslaufen lassen; Sifa wechselweise bedienen.
    let mut v_last = sim.trains[t].speed_kmh();
    for i in 0..12_000 {
        hold_sifa(&mut sim, t, (i / 200) % 2 == 0);
        sim.step(Sim::DT);
    }
    let v_end = sim.trains[t].speed_kmh();
    assert!(v_end < v_last, "Zug muss auslaufen");

    // Sollverzögerung aus Davis: a = R(v)/m_träge, auf gerader Strecke.
    let train = &sim.trains[t];
    // Sollwert bei mittlerer Geschwindigkeit des Auslaufs.
    let v_mean = (120.0 + v_end) / 2.0 / 3.6;
    let r: f64 = train
        .vehicles
        .iter()
        .map(|v| v.spec.davis.resistance(v_mean))
        .sum();
    let m: f64 = train.vehicles.iter().map(|v| v.inertial_mass()).sum();
    let a_soll = r / m;
    let a_ist = (120.0 - v_end) / 3.6 / 60.0;
    assert!(
        (a_ist - a_soll).abs() / a_soll < 0.15,
        "Auslaufverzögerung {a_ist:.4} vs Davis {a_soll:.4} m/s²"
    );
    v_last = v_end;
    assert!(v_last > 90.0, "Auslauf viel zu stark: {v_last} km/h");
}

#[test]
fn schnellbremsung_aus_100_kmh_trifft_bremstafel() {
    let mut sim = new_sim();
    let t = passenger_train(&mut sim, 5);
    power_up(&mut sim, t);
    set_speed(&mut sim, t, 100.0);

    let brh = sim.trains[t].brake_percentage();
    assert!(
        (100.0..=160.0).contains(&brh),
        "Bremshundertstel unplausibel: {brh}"
    );

    let start = sim.runtime[t].odometer;
    sim.controls[t].brake_valve = DriverBrakeValve::Emergency;
    for i in 0..24_000 {
        hold_sifa(&mut sim, t, (i / 200) % 2 == 0);
        sim.step(Sim::DT);
        if sim.trains[t].speed_kmh() < 0.5 {
            break;
        }
    }
    let distance = sim.runtime[t].odometer - start;
    assert!(sim.trains[t].speed_kmh() < 1.0, "Zug muss stehen");
    // Bremstafel: bei ~130 Bremshundertsteln liegt der Schnellbremsweg aus 100 km/h
    // in der Größenordnung 400–500 m. Toleranz großzügig, aber nicht beliebig.
    assert!(
        (300.0..=650.0).contains(&distance),
        "Schnellbremsweg {distance:.0} m außerhalb des Erwartungsbereichs"
    );
}

#[test]
fn gueterzug_bremst_hinten_spaeter() {
    let mut sim = new_sim();
    let head = TrackPosition::new(EdgeId(0), 100.0, 1);
    let mut vehicles: Vec<Vehicle> = vec![vehicle(br101(), head, SafetySystems::None)];
    for _ in 0..25 {
        vehicles.push(vehicle(freight_wagon(), head, SafetySystems::None));
    }
    let train = Train::assemble(vehicles, head, &sim.net);
    let t = sim.add_train(train);
    set_speed(&mut sim, t, 60.0);

    sim.controls[t].brake_valve = DriverBrakeValve::Emergency;
    // Eine halbe Sekunde bremsen und die Druckwelle beobachten.
    for _ in 0..100 {
        sim.step(Sim::DT);
    }
    let front = sim.trains[t].vehicles[1].brake.pipe;
    let back = sim.trains[t].vehicles.last().unwrap().brake.pipe;
    assert!(
        back > front + 0.2,
        "Druck hinten ({back:.2} bar) muss dem vorderen ({front:.2} bar) nachlaufen"
    );
    // Und am Ende bremst der ganze Zug — Bremsstellung G braucht dafür ~ 22 s.
    for _ in 0..12_000 {
        sim.step(Sim::DT);
    }
    let back_cyl = sim.trains[t].vehicles.last().unwrap().brake.cylinder;
    assert!(
        back_cyl > 3.0,
        "letzter Wagen muss angelegt haben: {back_cyl}"
    );
}

#[test]
fn anfahren_am_berg_und_kraftschlussgrenze() {
    let mut sim = new_sim();
    // Auf die 8-‰-Steigung im dritten Abschnitt setzen.
    let head = TrackPosition::new(EdgeId(2), 1000.0, 1);
    let mut vehicles = vec![vehicle(br101(), head, SafetySystems::None)];
    for _ in 0..8 {
        vehicles.push(vehicle(passenger_coach(), head, SafetySystems::None));
    }
    let train = Train::assemble(vehicles, head, &sim.net);
    let t = sim.add_train(train);
    power_up(&mut sim, t);

    // Ohne Zugkraft rollt der Zug rückwärts.
    sim.controls[t].brake_valve = DriverBrakeValve::Release;
    for _ in 0..2000 {
        sim.step(Sim::DT);
    }
    assert!(
        sim.trains[t].speed() < -0.05,
        "am Berg ohne Zugkraft muss der Zug zurückrollen: {} m/s",
        sim.trains[t].speed()
    );

    // Mit voller Zugkraft fährt er an.
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 1.0;
    for _ in 0..6000 {
        sim.step(Sim::DT);
    }
    assert!(
        sim.trains[t].speed_kmh() > 5.0,
        "Anfahren am Berg gescheitert: {} km/h",
        sim.trains[t].speed_kmh()
    );

    // Kraftschluss: die Lok überträgt nie mehr als µ·m·g.
    let lok = &sim.trains[t].vehicles[0];
    let mu = sim_core::physics::adhesion_coefficient(lok.v * 3.6, sim.trains[t].rail, false);
    let limit = mu * lok.adhesive_mass() * sim_core::G;
    assert!(
        lok.tractive_effort <= limit * 1.05,
        "übertragene Zugkraft {} N über der Kraftschlussgrenze {} N",
        lok.tractive_effort,
        limit
    );
}

#[test]
fn zug_streckt_sich_beim_anfahren() {
    let mut sim = new_sim();
    let t = passenger_train(&mut sim, 5);
    power_up(&mut sim, t);
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 0.6;
    let mut max_tension: f64 = 0.0;
    for _ in 0..3000 {
        sim.step(Sim::DT);
        max_tension = max_tension.max(sim.trains[t].couplers[0].force);
    }
    assert!(max_tension > 0.0, "erste Kupplung muss auf Zug gehen");
    // Kupplungsspiel: die hinteren Fahrzeuge setzen sich später in Bewegung.
    assert!(
        sim.trains[t].vehicles[0].x > sim.trains[t].vehicles[5].x,
        "Zug muss gestreckt sein"
    );
    assert!(
        sim.trains[t].couplers[0].extension > 0.0,
        "Kupplung gedehnt"
    );
    // Keine Kupplung darf beim normalen Anfahren reißen.
    assert!(sim.trains[t].couplers.iter().all(|c| !c.broken));
}

#[test]
fn determinismus_zwei_laeufe_gleicher_hash() {
    let run = || {
        let mut sim = new_sim();
        let t = passenger_train(&mut sim, 5);
        power_up(&mut sim, t);
        sim.controls[t].reverser = 1;
        for i in 0..4000 {
            sim.controls[t].throttle = if i < 2000 { 0.8 } else { 0.0 };
            sim.controls[t].brake_valve = if i > 3000 {
                DriverBrakeValve::Service(0.8)
            } else {
                DriverBrakeValve::Release
            };
            hold_sifa(&mut sim, t, (i / 200) % 2 == 0);
            sim.step(Sim::DT);
        }
        sim.state_hash()
    };
    assert_eq!(
        run(),
        run(),
        "gleicher Seed muss identischen Zustand liefern"
    );
}

#[test]
fn save_load_roundtrip_erhaelt_zustand() {
    let mut sim = new_sim();
    let t = passenger_train(&mut sim, 3);
    power_up(&mut sim, t);
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 0.7;
    for _ in 0..2000 {
        sim.step(Sim::DT);
    }
    let hash = sim.state_hash();
    let text = ron::ser::to_string_pretty(&sim, ron::ser::PrettyConfig::default()).unwrap();
    let mut restored: Sim = ron::from_str(&text).expect("Sim lesbar");
    restored.net.finish();
    assert_eq!(restored.state_hash(), hash);

    // Und weiterrechnen liefert dasselbe wie im Original.
    for _ in 0..500 {
        sim.step(Sim::DT);
        restored.step(Sim::DT);
    }
    assert_eq!(restored.state_hash(), sim.state_hash());
}
