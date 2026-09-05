//! Wind turbines: where they stand, what they are, and how big to build them.
//!
//! A [`crate::route::WindTurbineSource`] is a point on the ground plus the two
//! numbers a viewer perceives — the hub height and the rotor diameter. The
//! machine's name comes with it, but only as a label: from a train, a 2 MW
//! Enercon and a 2 MW Vestas are a white tube with three blades on it, and what
//! tells them apart at that distance is how tall and how wide they are.
//!
//! **Two sources, and each answers what the other cannot.** OpenStreetMap has
//! the position, surveyed by people who walked past it, and about a third of
//! the machines' names; the Bundesnetzagentur's Marktstammdatenregister has the
//! manufacturer, the type, the hub height and the rotor diameter of every unit
//! in the country, and a position from the permit. So the import takes the
//! geometry from OSM and the machine from the register, matched by the
//! `ref:mastr` the mappers write or, failing that, by distance
//! ([`match_register`]). In a test box of 78 turbines, 75 carried the reference
//! and the nearest-unit match agreed with every one of them.
//!
//! **A turbine is a scenery object, not a tree.** A mast rides in with the
//! vegetation because it never moves; a turbine's nacelle yaws with the wind
//! and its rotor turns, and a thing with moving parts has to be a scene with
//! named nodes rather than a flattened instance. So [`placements`] hands the
//! turbines to the tile pipeline as geo-positioned scenery
//! ([`crate::terrain::GeoObject`]), the same path a hut or a signal box takes,
//! and `world_render::wind` moves the nodes by name.
//!
//! **The models are `mods/wind`**, generated from `tools/wind/wind.json`: one
//! per size class and build, four levels of detail each. [`PRESETS`] is a copy
//! of the catalogue's dimensions, so a class picks the model built nearest to
//! the machine and the placement scales it to the machine; the build follows
//! the maker — Enercon's drop nacelle and green tower foot, a lattice tower
//! under a Fuhrländer, the box nacelle everyone else builds ([`object_for`]).
//!
//! Nothing here is simulated. A wind turbine is scenery: no state, nothing to
//! replicate, and both clients of a multiplayer run build the same turbines out
//! of the same line file. Even the movement is not sent — a rotor turns because
//! the weather says so (`sim_core::weather::Weather`), and the weather is
//! already shared.

use crate::route::WindTurbineSource;
use crate::terrain::GeoObject;
use fields::mastr::WindUnit;

/// Which way the nacelles look [deg from north, clockwise] — the direction the
/// rotor faces into, which is the direction the wind comes *from*.
///
/// Nothing surveys this. A turbine yaws into the wind and keeps no direction of
/// its own, so there is no true value to import; what there is, is a
/// prevailing wind, and over Germany it is a westerly to south-westerly. 250°
/// is the middle of that.
///
/// It is the same value for every turbine of an import on purpose. Wind is a
/// weather-scale thing: every machine within sight of a train stands in the
/// same air and points the same way, and a park whose rotors face at random is
/// the one thing a viewer reads as wrong immediately.
pub const PREVAILING_BEARING: f64 = 250.0;

/// How far a turbine may be from a register unit and still be the same machine
/// [m].
///
/// The two positions are a survey and a permit drawing, and in a test box of 78
/// turbines they differed by 1.9 m in the median and 16 m at the ninth decile,
/// with one outlier at 98 m. German turbines stand hundreds of metres apart —
/// the rotors would otherwise take each other's wind — so a hundred metres is
/// wide enough for the outlier and far too narrow to reach the neighbour.
const MATCH_RADIUS: f64 = 100.0;

/// The rotor a machine has to reach to count as a wind turbine of the kind a
/// landscape is made of [m].
///
/// Below it is a Kleinwindanlage: the mast in a farmyard or beside a workshop,
/// a few tens of kilowatts and under thirty metres to the tip. There are many
/// of them, they are furniture rather than landscape, and a module usually
/// wants the ones that stand on the horizon — so the import leaves them out
/// unless it is asked for them. The smallest machine in the register's coastal
/// box is a 50 kW one with a 15 m rotor.
pub const SMALL_ROTOR: f64 = 20.0;

/// Whether a turbine is one of the small ones — see [`SMALL_ROTOR`].
pub fn is_small(turbine: &WindTurbineSource) -> bool {
    turbine.rotor_diameter < SMALL_ROTOR
}

/// A size class of turbine: what the model is built at, and which machines land
/// on it.
///
/// The classes are the German fleet's own generations, and the dimensions are
/// the medians measured in the register over three regions (the Dithmarschen
/// coast, the Magdeburger Börde and the Hunsrück): the 1990s machines around a
/// 50 m rotor, the 2000s workhorses around 80 m, the 2010s around 115 m and
/// what is being built now around 150 m and up. `tools/wind/wind.json` is the
/// same table, and the models are built at exactly these numbers.
#[derive(Debug, Clone, Copy)]
pub struct WindPreset {
    /// The class id, as it goes into the source's tags (`wea-115`).
    pub id: &'static str,
    /// The object stem (`"wind:wea_115"`); the build's suffix goes on the end
    /// (see [`object_for`]).
    pub object: &'static str,
    /// Hub height the model is built at [m].
    pub hub: f64,
    /// Rotor diameter the model is built at [m].
    pub rotor: f64,
    /// The largest rotor diameter that still lands on this class [m]; the last
    /// class takes everything above.
    pub up_to: f64,
    /// Whether the class is built on a lattice tower as well — only the small
    /// generations were.
    pub lattice: bool,
}

/// The size classes, smallest first — `mods/wind`, one file per class and
/// build.
pub const PRESETS: &[WindPreset] = &[
    WindPreset {
        id: "wea-50",
        object: "wind:wea_50",
        hub: 65.0,
        rotor: 50.0,
        up_to: 60.0,
        lattice: true,
    },
    WindPreset {
        id: "wea-80",
        object: "wind:wea_80",
        hub: 95.0,
        rotor: 80.0,
        up_to: 100.0,
        lattice: true,
    },
    WindPreset {
        id: "wea-115",
        object: "wind:wea_115",
        hub: 125.0,
        rotor: 115.0,
        up_to: 130.0,
        lattice: false,
    },
    WindPreset {
        id: "wea-150",
        object: "wind:wea_150",
        hub: 140.0,
        rotor: 150.0,
        up_to: f64::INFINITY,
        lattice: false,
    },
];

/// The build of a class a machine gets, by who made it.
///
/// From a train the makers differ in one thing each: Enercon's nacelle is a
/// drop and its tower foot is ringed in green, and a Fuhrländer of the
/// nineties stands on a lattice tower. Everyone else — Vestas, Nordex,
/// Senvion, GE, Siemens — builds a box on a tube, and the box is the default
/// for a machine nobody could name.
pub fn object_for(class: &WindPreset, model: &str) -> String {
    let name = model.to_lowercase();
    let build = if name.contains("enercon") {
        "enercon"
    } else if class.lattice && name.contains("fuhrl") {
        "gitter"
    } else {
        "standard"
    };
    format!("{}_{build}", class.object)
}

/// The class of an id.
pub fn preset(id: &str) -> Option<&'static WindPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// The class a rotor of this size lands on. The rotor decides, not the tower:
/// two machines with the same rotor are the same generation of machine however
/// high the site made them build it.
pub fn class_for(rotor_diameter: f64) -> &'static WindPreset {
    PRESETS
        .iter()
        .find(|p| rotor_diameter <= p.up_to)
        .unwrap_or(&PRESETS[PRESETS.len() - 1])
}

/// The dimensions a turbine of this rated power has [m], `(hub, rotor)` — the
/// fallback for a machine neither source gave numbers for.
///
/// The rotor comes out of the **specific power**, the rated power per square
/// metre of swept area, which is what a turbine is designed around: over 387
/// operating machines in the register it is 335 W/m² in the median and moves
/// between 290 and 480 across the whole fleet, from the 1990s 600 kW machines
/// to today's 6 MW ones. The tower then follows the rotor — `40 + 0.75 d` is
/// the middle of the three regional fits, which run from `29 + 0.64 d` on the
/// windy coast, where a short tower will do, to `60 + 0.70 d` in the Hunsrück,
/// where the machine has to reach over a forest.
///
/// The spread of that is real: the same 112 m rotor sits at 94 m on the coast
/// and at 140 m in the low mountains. It is also why the register is asked at
/// all — with it, this is the answer for the odd unit that has no numbers, and
/// without it, it is the answer for most of them.
pub fn estimate(power_kw: f64) -> (f64, f64) {
    const SPECIFIC_POWER: f64 = 335.0;
    let rotor = if power_kw > 0.0 {
        (4.0 * power_kw * 1000.0 / (std::f64::consts::PI * SPECIFIC_POWER)).sqrt()
    } else {
        // Nothing known at all: the machine the German landscape is fullest
        // of, a 2 MW class turbine of the 2000s.
        80.0
    };
    (40.0 + 0.75 * rotor, rotor)
}

/// A turbine source stamped from what the sources said — what the OSM import
/// produces and [`match_register`] corrects.
///
/// `estimated` says the dimensions are worked out from the rated power rather
/// than known, and it goes into the tags so the file says which of its numbers
/// were surveyed.
pub fn source_from(
    lat: f64,
    lon: f64,
    hub_height: f64,
    rotor_diameter: f64,
    model: String,
    mastr: String,
    estimated: bool,
) -> WindTurbineSource {
    let class = class_for(rotor_diameter);
    let mut tags = vec![class.id.to_string()];
    if estimated {
        tags.push("estimated".to_string());
    }
    WindTurbineSource {
        lat,
        lon,
        hub_height,
        rotor_diameter,
        object: object_for(class, &model),
        yaw_deg: PREVAILING_BEARING,
        model,
        mastr,
        tags,
    }
}

/// What [`match_register`] made of the register's answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RegisterMatch {
    /// Turbines the register named a machine for.
    pub matched: usize,
    /// Standing units of the register that no turbine claimed — the ones
    /// OpenStreetMap has not mapped yet, or that stand outside the box the
    /// turbines were read from.
    pub spare: usize,
}

/// Fills in what the register knows about the turbines: the machine's name, its
/// hub height and its rotor diameter, and with them the size class and the
/// object.
///
/// A turbine that carries a `ref:mastr` from OpenStreetMap is matched on it and
/// on nothing else — the mapper read the number off the tower. The rest take
/// the nearest unit still free within [`MATCH_RADIUS`], and only a unit that is
/// still standing: a decommissioned one is why the register has a turbine the
/// map does not, not what the map is looking at.
///
/// A turbine the register cannot answer for keeps what it came with, estimate
/// and all.
pub fn match_register(turbines: &mut [WindTurbineSource], units: &[WindUnit]) -> RegisterMatch {
    let mut taken = vec![false; units.len()];
    let mut matched = 0;

    // The references first, so a proximity match cannot take a unit that a
    // mapper has already pinned to another turbine.
    for turbine in turbines.iter_mut().filter(|t| !t.mastr.is_empty()) {
        if let Some(i) = units.iter().position(|u| u.mastr == turbine.mastr)
            && !taken[i]
        {
            taken[i] = true;
            apply(turbine, &units[i]);
            matched += 1;
        }
    }

    for turbine in turbines.iter_mut() {
        if turbine.tags.iter().any(|t| t == "mastr") {
            continue;
        }
        let best = units
            .iter()
            .enumerate()
            .filter(|(i, u)| !taken[*i] && u.status.standing())
            .map(|(i, u)| (metres(turbine.lat, turbine.lon, u.lat, u.lon), i))
            .filter(|(d, _)| *d <= MATCH_RADIUS)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, i)) = best {
            taken[i] = true;
            apply(turbine, &units[i]);
            matched += 1;
        }
    }

    let spare = units
        .iter()
        .zip(&taken)
        .filter(|(u, taken)| !**taken && u.status.standing())
        .count();
    RegisterMatch { matched, spare }
}

/// Writes a register unit onto a turbine: the machine's name and number always,
/// the dimensions where the register has them, and with them the class and the
/// object the class names.
fn apply(turbine: &mut WindTurbineSource, unit: &WindUnit) {
    turbine.mastr = unit.mastr.clone();
    let name = match (unit.manufacturer.as_str(), unit.model.as_str()) {
        ("", "") => String::new(),
        ("", model) => model.to_string(),
        (make, "") => make.to_string(),
        (make, model) => format!("{make} {model}"),
    };
    if !name.is_empty() {
        turbine.model = name;
    }
    let known = unit.hub_height > 0.0 && unit.rotor_diameter > 0.0;
    if known {
        turbine.hub_height = unit.hub_height;
        turbine.rotor_diameter = unit.rotor_diameter;
        turbine.tags.retain(|t| t != "estimated");
    }
    let class = class_for(turbine.rotor_diameter);
    turbine.object = object_for(class, &turbine.model);
    turbine.tags.retain(|t| preset(t).is_none() && t != "mastr");
    turbine.tags.insert(0, class.id.to_string());
    turbine.tags.push("mastr".to_string());
}

/// The line's wind turbines, as geo-positioned scenery.
///
/// A turbine without an object is passed over — a hand-edited file may say so
/// on purpose. The scale is the one that misses both dimensions by the same
/// share rather than getting one right and the other badly wrong: a placement
/// carries a single uniform scale, and a machine is never exactly its class —
/// a 101 m rotor on a 140 m tower is a class of 115 m and 125 m. So the
/// geometric mean of the two ratios decides ([`scale_of`]).
pub fn placements(list: &[WindTurbineSource]) -> Vec<GeoObject> {
    list.iter()
        .filter(|t| !t.object.is_empty())
        .map(|t| GeoObject {
            object: t.object.clone(),
            lat: t.lat,
            lon: t.lon,
            yaw_deg: t.yaw_deg,
            scale: scale_of(t),
        })
        .collect()
}

/// How much bigger than its class a turbine is — see [`turbines`].
pub fn scale_of(turbine: &WindTurbineSource) -> f64 {
    let class = class_for(turbine.rotor_diameter);
    let hub = if turbine.hub_height > 0.0 {
        turbine.hub_height / class.hub
    } else {
        1.0
    };
    let rotor = if turbine.rotor_diameter > 0.0 {
        turbine.rotor_diameter / class.rotor
    } else {
        1.0
    };
    (hub * rotor).sqrt()
}

/// The distance between two points [m]. Flat-earth over the hundred metres a
/// match may span, which is exact to a millimetre there.
fn metres(lat_a: f64, lon_a: f64, lat_b: f64, lon_b: f64) -> f64 {
    const DEG: f64 = 111_320.0;
    let mean = ((lat_a + lat_b) / 2.0).to_radians();
    let east = (lon_b - lon_a) * mean.cos() * DEG;
    let north = (lat_b - lat_a) * DEG;
    east.hypot(north)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fields::mastr::Status;

    fn unit(mastr: &str, lat: f64, lon: f64, hub: f64, rotor: f64, status: Status) -> WindUnit {
        WindUnit {
            mastr: mastr.to_string(),
            lat,
            lon,
            manufacturer: "Enercon".into(),
            model: "E-115 EP3".into(),
            hub_height: hub,
            rotor_diameter: rotor,
            power_kw: 3000.0,
            status,
            park: String::new(),
        }
    }

    #[test]
    fn the_rotor_picks_the_class() {
        assert_eq!(class_for(44.0).id, "wea-50");
        assert_eq!(class_for(60.0).id, "wea-50");
        assert_eq!(class_for(82.0).id, "wea-80");
        assert_eq!(class_for(112.0).id, "wea-115");
        assert_eq!(class_for(149.0).id, "wea-150");
        assert_eq!(class_for(240.0).id, "wea-150");
        // A machine of no known size is still a machine.
        assert_eq!(class_for(0.0).id, "wea-50");
    }

    #[test]
    fn a_rated_power_gives_a_size_worth_believing() {
        // The register's own numbers for these machines: a 2 MW class turbine
        // has a rotor of 70 to 82 m, a 3.45 MW one 112 m, a 5.7 MW one 149 m.
        let (hub, rotor) = estimate(2000.0);
        assert!((70.0..90.0).contains(&rotor), "2 MW rotor {rotor}");
        assert!((90.0..110.0).contains(&hub), "2 MW hub {hub}");
        let (_, rotor) = estimate(3450.0);
        assert!((100.0..120.0).contains(&rotor), "3.45 MW rotor {rotor}");
        let (_, rotor) = estimate(5700.0);
        assert!((135.0..155.0).contains(&rotor), "5.7 MW rotor {rotor}");
        // Nothing known: the machine the country is fullest of.
        assert_eq!(estimate(0.0).1, 80.0);
    }

    #[test]
    fn the_register_names_the_machine_by_reference() {
        let mut turbines = vec![source_from(
            52.0,
            10.0,
            95.0,
            80.0,
            String::new(),
            "SEE1".into(),
            true,
        )];
        // The unit sits far away — the reference is what matches it, not the
        // distance, because a mapper read the number off the tower.
        let units = vec![unit("SEE1", 52.05, 10.05, 149.0, 115.0, Status::Operating)];
        let report = match_register(&mut turbines, &units);
        assert_eq!(report.matched, 1);
        assert_eq!(report.spare, 0);
        let turbine = &turbines[0];
        assert_eq!(turbine.model, "Enercon E-115 EP3");
        assert_eq!(turbine.hub_height, 149.0);
        assert_eq!(turbine.rotor_diameter, 115.0);
        assert_eq!(turbine.tags, vec!["wea-115", "mastr"]);
    }

    #[test]
    fn without_a_reference_the_nearest_standing_unit_answers() {
        let mut turbines = vec![source_from(
            52.0,
            10.0,
            95.0,
            80.0,
            String::new(),
            String::new(),
            true,
        )];
        let units = vec![
            // Twenty metres away, but taken down: the map is not looking at it.
            unit("SEE-old", 52.0002, 10.0, 65.0, 48.0, Status::Decommissioned),
            // Forty metres away and turning.
            unit("SEE-now", 52.0004, 10.0, 125.0, 112.0, Status::Operating),
            // A kilometre away — the next turbine's business, not this one's.
            unit("SEE-far", 52.01, 10.0, 125.0, 112.0, Status::Operating),
        ];
        let report = match_register(&mut turbines, &units);
        assert_eq!(report.matched, 1);
        assert_eq!(turbines[0].mastr, "SEE-now");
        assert_eq!(turbines[0].rotor_diameter, 112.0);
        // The one standing unit nobody claimed; the decommissioned one is not
        // counted, because it is not there.
        assert_eq!(report.spare, 1);
    }

    #[test]
    fn a_unit_is_claimed_once() {
        let mut turbines = vec![
            source_from(52.0, 10.0, 95.0, 80.0, String::new(), String::new(), true),
            source_from(
                52.0002,
                10.0,
                95.0,
                80.0,
                String::new(),
                String::new(),
                true,
            ),
        ];
        let units = vec![unit(
            "SEE-1",
            52.0001,
            10.0,
            125.0,
            112.0,
            Status::Operating,
        )];
        let report = match_register(&mut turbines, &units);
        assert_eq!(report.matched, 1);
        assert_eq!(turbines[0].mastr, "SEE-1");
        assert!(turbines[1].mastr.is_empty());
        // The one that found nothing keeps what it came with.
        assert!(turbines[1].tags.iter().any(|t| t == "estimated"));
    }

    #[test]
    fn the_maker_picks_the_build() {
        let big = preset("wea-115").expect("class");
        assert_eq!(object_for(big, "Enercon E-101"), "wind:wea_115_enercon");
        assert_eq!(object_for(big, "Vestas V112"), "wind:wea_115_standard");
        assert_eq!(object_for(big, ""), "wind:wea_115_standard");
        // A lattice tower is a thing of the small generations only.
        let small = preset("wea-50").expect("class");
        assert_eq!(object_for(small, "Fuhrländer FL 600"), "wind:wea_50_gitter");
        assert_eq!(
            object_for(big, "Fuhrländer FL 2500"),
            "wind:wea_115_standard"
        );
    }

    #[test]
    fn a_turbine_is_placed_as_scenery() {
        let turbines_in = vec![source_from(
            52.0,
            10.0,
            125.0,
            112.0,
            "Vestas V112".into(),
            String::new(),
            false,
        )];
        // The placement is a geo-positioned scenery object — the scale off
        // the class, the yaw into the wind.
        let placed = placements(&turbines_in);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].object, "wind:wea_115_standard");
        assert_eq!(placed[0].yaw_deg, PREVAILING_BEARING);
        assert!(
            (placed[0].scale - 0.987).abs() < 0.01,
            "{}",
            placed[0].scale
        );

        // A file that names no object places nothing.
        let mut bare = turbines_in.clone();
        bare[0].object.clear();
        assert!(placements(&bare).is_empty());
    }

    #[test]
    fn a_machine_bigger_than_its_class_is_drawn_bigger() {
        let small = source_from(52.0, 10.0, 95.0, 80.0, String::new(), String::new(), false);
        let tall = source_from(
            52.0,
            10.0,
            140.0,
            100.0,
            String::new(),
            String::new(),
            false,
        );
        assert!(scale_of(&tall) > scale_of(&small));
        // The mean of the two ratios, not one of them: 140/95 is 1.47 and
        // 100/80 is 1.25, and neither alone is the answer.
        assert!(
            (scale_of(&tall) - 1.356).abs() < 0.01,
            "{}",
            scale_of(&tall)
        );
    }
}
