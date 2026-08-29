//! Which federal state a place belongs to, and what that state publishes.
//!
//! Germany devolves agriculture to the states, so there is no national field
//! service — there are sixteen, on two levels of detail, under four different
//! licences, with a crop code list each. Everything the import needs to know
//! about a state sits here: where its boundary runs, which service answers,
//! what comes back and what has to be written under the picture afterwards.
//!
//! The boundaries are the BKG's VG2500, thinned to about half a kilometre and
//! baked into `laender.bin` by `tools/gen_laender.py`. Half a kilometre is
//! coarse for a border, and deliberately so: [`Land::touching`] asks every
//! state a query box comes near, so being unsure at the border costs one extra
//! request rather than a wrong service.

use std::sync::OnceLock;

/// The sixteen states, in the order `laender.bin` stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Land {
    Bw,
    By,
    Be,
    Bb,
    Hb,
    Hh,
    He,
    Mv,
    Ni,
    Nw,
    Rp,
    Sl,
    Sn,
    St,
    Sh,
    Th,
}

/// How much a state's service says about a parcel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// GSA: the applied-for parcel, with its crop code.
    Gsa,
    /// LPIS: the field block, arable or grassland and no more.
    Lpis,
    /// No register at all, so OpenStreetMap — where somebody has walked it
    /// (plan ch. 3).
    Osm,
    /// Nothing to be had.
    None,
}

/// What has to be written under a picture that uses the data.
///
/// The four licences the states use are not interchangeable. `dl-de/zero-2-0`
/// asks for nothing, `dl-de/by-2-0` and `CC BY 4.0` want the source named, and
/// Bavaria's *Feldstückskarte* is `CC BY-ND` — no derivatives, which is exactly
/// what turning a polygon into a mesh is, so that one is not used at all and
/// the LPIS service under `CC BY` is asked instead (plan ch. 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Licence {
    /// dl-de/by-2-0 — Datenlizenz Deutschland, Namensnennung.
    DlDeBy20,
    /// dl-de/zero-2-0 — no attribution required.
    DlDeZero20,
    /// CC BY 4.0.
    CcBy40,
    /// ODbL — OpenStreetMap. Share-alike: a module built on it carries the
    /// obligation on, which is why it is the fallback and not the first choice
    /// (see [`crate::osm`]).
    Odbl,
    /// The state has not stated one. Mecklenburg-Vorpommern says "UrhG",
    /// Saxony-Anhalt says nothing, Baden-Württemberg has no open download at
    /// all: those have to be settled in writing before a line ships (plan
    /// ch. 2, ch. 9), and the import says so rather than quietly using them.
    Unclear,
}

impl Licence {
    /// Whether a line that used this data has to carry a source note.
    pub fn needs_attribution(self) -> bool {
        !matches!(self, Licence::DlDeZero20)
    }

    /// The short name, as it is written in a credits list — a licence
    /// identifier, not prose, so it is the same in every language.
    pub fn id(self) -> &'static str {
        match self {
            Licence::DlDeBy20 => "dl-de/by-2-0",
            Licence::DlDeZero20 => "dl-de/zero-2-0",
            Licence::CcBy40 => "CC BY 4.0",
            Licence::Odbl => "ODbL 1.0",
            Licence::Unclear => "?",
        }
    }
}

/// How a state's service is talked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// WFS `GetFeature` with a bounding box, GeoJSON back — the whole state
    /// never has to be downloaded.
    Wfs,
    /// The parcels come as points, not polygons: Saxony publishes the GSA that
    /// way, so the crop is read off the point and the shape off the LPIS
    /// polygon it falls in (see [`Service::join`]).
    WfsPointJoin,
    /// Only a whole-state file exists. The import cannot fetch it in the
    /// background — the user downloads it once and points the editor at it.
    Download,
    /// No register, so OpenStreetMap instead — `landuse=farmland` and its
    /// neighbours, with the crop drawn from the statistics (see [`crate::osm`]).
    Osm,
    /// Nothing to ask.
    None,
}

/// Everything needed to ask one state for its fields.
#[derive(Debug, Clone, Copy)]
pub struct Service {
    pub level: Level,
    pub access: Access,
    /// WFS endpoint, or the page the file is downloaded from.
    pub url: &'static str,
    /// Type name of the parcel layer.
    pub layer: &'static str,
    /// Type name of the polygon layer a [`Access::WfsPointJoin`] joins against.
    pub join: &'static str,
    /// `OUTPUTFORMAT` the endpoint wants — the spelling differs per server.
    pub format: &'static str,
    pub licence: Licence,
    /// Who the licence says to name, verbatim and without the licence itself —
    /// [`crate::Attribution`] puts the two together. Empty where no note is
    /// needed. A proper name, so it is not translated.
    pub credit: &'static str,
}

impl Service {
    /// Nothing published for this state.
    const fn none(licence: Licence) -> Self {
        Self {
            level: Level::None,
            access: Access::None,
            url: "",
            layer: "",
            join: "",
            format: "",
            licence,
            credit: "",
        }
    }
}

impl Land {
    pub const ALL: [Land; 16] = [
        Land::Bw,
        Land::By,
        Land::Be,
        Land::Bb,
        Land::Hb,
        Land::Hh,
        Land::He,
        Land::Mv,
        Land::Ni,
        Land::Nw,
        Land::Rp,
        Land::Sl,
        Land::Sn,
        Land::St,
        Land::Sh,
        Land::Th,
    ];

    /// The ISO 3166-2 suffix — `NW`, `BY`. An identifier, not prose.
    pub fn code(self) -> &'static str {
        match self {
            Land::Bw => "BW",
            Land::By => "BY",
            Land::Be => "BE",
            Land::Bb => "BB",
            Land::Hb => "HB",
            Land::Hh => "HH",
            Land::He => "HE",
            Land::Mv => "MV",
            Land::Ni => "NI",
            Land::Nw => "NW",
            Land::Rp => "RP",
            Land::Sl => "SL",
            Land::Sn => "SN",
            Land::St => "ST",
            Land::Sh => "SH",
            Land::Th => "TH",
        }
    }

    pub fn from_code(code: &str) -> Option<Land> {
        Land::ALL.into_iter().find(|l| l.code() == code)
    }

    /// The state's name. A place name, so it reads the same in every language.
    pub fn name(self) -> &'static str {
        match self {
            Land::Bw => "Baden-Württemberg",
            Land::By => "Bayern",
            Land::Be => "Berlin",
            Land::Bb => "Brandenburg",
            Land::Hb => "Bremen",
            Land::Hh => "Hamburg",
            Land::He => "Hessen",
            Land::Mv => "Mecklenburg-Vorpommern",
            Land::Ni => "Niedersachsen",
            Land::Nw => "Nordrhein-Westfalen",
            Land::Rp => "Rheinland-Pfalz",
            Land::Sl => "Saarland",
            Land::Sn => "Sachsen",
            Land::St => "Sachsen-Anhalt",
            Land::Sh => "Schleswig-Holstein",
            Land::Th => "Thüringen",
        }
    }

    /// The UTM zone the state's own services publish in — 32 west of about
    /// 12° E, 33 east of it. Germany's convention, not a computation: Saxony
    /// and Brandenburg deliver 25833 even where they reach into zone 32.
    pub fn utm_zone(self) -> u8 {
        match self {
            Land::Bb | Land::Be | Land::Mv | Land::Sn | Land::St => 33,
            _ => 32,
        }
    }

    /// Where the fields of this state come from.
    //
    // ponytail: a table in code rather than a configuration file. The endpoints
    // change about once a year and a wrong one is a bug report, not a setting;
    // the BLE keeps the register that says when they move
    // (https://gdi.bmleh.de/geodaten/geodaten-aus-dem-invekos-eu-agrarfoerderung),
    // and looking there belongs in the release routine.
    pub fn service(self) -> Service {
        match self {
            // Metadata records exist, no documented open download — the data
            // has to be asked for at the ministry (plan ch. 2).
            Land::Bw => Service::none(Licence::Unclear),

            // Only field blocks. The Feldstückskarte would have the parcels but
            // is CC BY-ND, which a mesh export breaks — so LPIS under CC BY.
            Land::By => Service {
                level: Level::Lpis,
                access: Access::Wfs,
                url: "https://gdiserv.bayern.de/srv66381/services/invekos_lpis-wfs",
                layer: "invekos_lpis:Feldstueck",
                join: "",
                format: "application/json",
                licence: Licence::CcBy40,
                credit: "Bayerische Vermessungsverwaltung / StMELF",
            },

            // Brandenburg and Berlin share the application data.
            Land::Bb | Land::Be => Service {
                level: Level::Gsa,
                access: Access::Wfs,
                url: "https://isk.geobasis-bb.de/ows/geobroker_l_dfbk_wfs",
                layer: "dfbk:dfbk",
                join: "",
                format: "application/json",
                licence: Licence::DlDeBy20,
                credit: "© GeoBasis-DE/LGB",
            },

            // The city states are served by Lower Saxony's SLA.
            Land::Hb | Land::Hh | Land::Ni => Service {
                level: Level::Gsa,
                access: Access::Wfs,
                url: "https://sla.niedersachsen.de/agrarfoerderung/agrar_ant_inspire/wfs",
                layer: "agrar_ant_inspire:GSA_AgriculturalParcel",
                join: "",
                format: "application/json",
                licence: Licence::CcBy40,
                credit: "SLA Niedersachsen",
            },

            Land::He => Service {
                level: Level::Lpis,
                access: Access::Wfs,
                url: "https://inspire-hessen.de/ows/services/org.548.11e0f9e5-4a4a-4bd8-a2ea-3fd1a9d0a3b8_wfs",
                layer: "lu:ExistingLandUseObject",
                join: "",
                format: "application/json",
                licence: Licence::CcBy40,
                credit: "Land Hessen / HVBG",
            },

            // "UrhG" in the BLE table — not an open licence, so the import
            // fetches it but marks the line as needing a written clearance.
            Land::Mv => Service {
                level: Level::Lpis,
                access: Access::Wfs,
                url: "https://www.geodaten-mv.de/dienste/gdimv_feldblock_wfs",
                layer: "mv:feldbloecke",
                join: "",
                format: "application/json",
                licence: Licence::Unclear,
                credit: "Landesamt für innere Verwaltung Mecklenburg-Vorpommern",
            },

            Land::Nw => Service {
                level: Level::Gsa,
                access: Access::Wfs,
                url: "https://www.wfs.nrw.de/umwelt/lwk_eufoerderung",
                layer: "umwelt_lwk_eufoerderung:Beantragte_und_als_foerderfaehig_festgestellte_Teilschlaege_in_NRW",
                join: "",
                format: "GEOJSON",
                licence: Licence::DlDeBy20,
                credit: "© Landwirtschaftskammer Nordrhein-Westfalen",
            },

            // No InVeKoS service at all: the BLE's register entry is empty and
            // the application portal is behind a login. OpenStreetMap instead,
            // with the crop drawn from the statistics (plan ch. 3).
            //
            // ponytail: the better fallback is the ATKIS Basis-DLM the state
            // publishes under dl-de/by-2-0 — surveyed geometry with
            // arable/grassland/vineyard on it. It comes out of a web shop as a
            // whole-state download rather than a service, so it wants the same
            // "point the editor at a file" path Schleswig-Holstein needs.
            Land::Rp => Service {
                level: Level::Osm,
                access: Access::Osm,
                url: crate::osm::OVERPASS,
                layer: "landuse",
                join: "",
                format: "",
                licence: Licence::Odbl,
                credit: "© OpenStreetMap contributors",
            },

            Land::Sl => Service {
                level: Level::Lpis,
                access: Access::Wfs,
                url: "https://geoportal.saarland.de/gdi-sl/inspire/wfs_lu",
                layer: "lu:ExistingLandUseObject",
                join: "",
                format: "application/json",
                licence: Licence::CcBy40,
                credit: "Landesamt für Vermessung, Geoinformation und Landentwicklung Saarland",
            },

            // The parcels are points; the shape comes from the reference
            // parcel they fall in.
            Land::Sn => Service {
                level: Level::Gsa,
                access: Access::WfsPointJoin,
                url: "https://geodienste.sachsen.de/ags/public_iwfs_gsz_invekos/MapServer/WFSServer",
                layer: "invekos:AgriculturalParcel",
                join: "invekos:ReferenceParcel",
                format: "GEOJSON",
                licence: Licence::DlDeBy20,
                credit: "© Freistaat Sachsen, LfULG",
            },

            Land::St => Service {
                level: Level::Lpis,
                access: Access::Wfs,
                url: "https://www.geodatenportal.sachsen-anhalt.de/wss/service/INSPIRE_LSA_MWL_LC_WFS/guest",
                layer: "lc:LandCoverUnit",
                join: "",
                format: "application/json",
                licence: Licence::Unclear,
                credit: "Land Sachsen-Anhalt (MWL)",
            },

            // A yearly GeoPackage, no WFS — the user fetches it once.
            Land::Sh => Service {
                level: Level::Lpis,
                access: Access::Download,
                url: "https://service.gdi-sh.de/SH_OpenGBD/feeds/Atom_SH_Feldblockfinder_OpenGBD/",
                layer: "Feldbloecke",
                join: "",
                format: "",
                licence: Licence::DlDeZero20,
                credit: "",
            },

            Land::Th => Service {
                level: Level::Gsa,
                access: Access::Wfs,
                url: "https://www.geoproxy.geoportal-th.de/geoproxy/services/agrar/feldblock_wfs",
                layer: "agrar:feldblock",
                join: "",
                format: "application/json",
                licence: Licence::DlDeBy20,
                credit: "© GDI-Th",
            },
        }
    }

    /// Whether `(lat, lon)` [deg] lies in this state.
    pub fn contains(self, lat: f64, lon: f64) -> bool {
        let Some(rings) = boundaries().get(self as usize) else {
            return false;
        };
        rings.iter().any(|ring| point_in_ring(lon, lat, ring))
    }

    /// The state a point lies in, if any — a point out at sea or over the
    /// border has none.
    pub fn at(lat: f64, lon: f64) -> Option<Land> {
        Land::ALL.into_iter().find(|l| l.contains(lat, lon))
    }

    /// Every state a bounding box in degrees reaches into.
    ///
    /// A query box is asked of all of them, not just of the one its centre
    /// falls in: a module on a state border holds fields of both, and the
    /// baked boundary is half a kilometre coarse anyway. A service that has
    /// nothing there simply answers with nothing.
    pub fn touching(min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64) -> Vec<Land> {
        let mut found = Vec::new();
        for land in Land::ALL {
            let Some(rings) = boundaries().get(land as usize) else {
                continue;
            };
            if rings
                .iter()
                .any(|ring| ring_meets_box(ring, min_lon, min_lat, max_lon, max_lat))
            {
                found.push(land);
            }
        }
        found
    }
}

/// The UTM zone a longitude falls in.
///
/// [`Land::utm_zone`] is a table because Germany's states publish in a zone by
/// convention rather than by geography — Saxony delivers 25833 even where it
/// reaches into zone 32. Outside Germany there is no such convention and no
/// state to ask, so the zone is the one the longitude is actually in.
pub fn utm_zone_at(lon: f64) -> u8 {
    (((lon + 180.0) / 6.0).floor() as i32).clamp(0, 59) as u8 + 1
}

/// One state's boundary: its outer rings, `(lon, lat)` [deg].
type Rings = Vec<Vec<(f32, f32)>>;

/// The thinned VG2500 rings per state, parsed once.
fn boundaries() -> &'static [Rings] {
    static RINGS: OnceLock<Vec<Rings>> = OnceLock::new();
    RINGS.get_or_init(|| parse(include_bytes!("laender.bin")))
}

/// Reads the layout `tools/gen_laender.py` writes. A file that does not parse
/// yields empty boundaries: the import then finds no state and says so, which
/// is a great deal better than a panic on startup.
fn parse(bytes: &[u8]) -> Vec<Rings> {
    let mut out = vec![Vec::new(); Land::ALL.len()];
    let mut at = 0usize;
    let u8_at = |at: usize| bytes.get(at).copied();
    let u16_at = |at: usize| Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
    let f32_at = |at: usize| {
        Some(f32::from_le_bytes([
            *bytes.get(at)?,
            *bytes.get(at + 1)?,
            *bytes.get(at + 2)?,
            *bytes.get(at + 3)?,
        ]))
    };
    let Some(states) = u8_at(at) else {
        return out;
    };
    at += 1;
    for _ in 0..states {
        let (Some(index), Some(rings)) = (u8_at(at), u16_at(at + 1)) else {
            return out;
        };
        at += 3;
        let mut shape = Vec::with_capacity(rings as usize);
        for _ in 0..rings {
            let Some(points) = u16_at(at) else {
                return out;
            };
            at += 2;
            let mut ring = Vec::with_capacity(points as usize);
            for _ in 0..points {
                let (Some(x), Some(y)) = (f32_at(at), f32_at(at + 4)) else {
                    return out;
                };
                ring.push((x, y));
                at += 8;
            }
            shape.push(ring);
        }
        if let Some(slot) = out.get_mut(index as usize) {
            *slot = shape;
        }
    }
    out
}

/// Ray casting in degrees. Good enough for a boundary that is itself thinned
/// to half a kilometre.
fn point_in_ring(x: f64, y: f64, ring: &[(f32, f32)]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (ax, ay) = (ring[i].0 as f64, ring[i].1 as f64);
        let (bx, by) = (ring[j].0 as f64, ring[j].1 as f64);
        if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Whether a ring and an axis-aligned box overlap at all: a corner of the box
/// inside the ring, a vertex of the ring inside the box, or an edge crossing.
/// The last case is what catches a box entirely inside one state's polygon
/// gap-free while its vertices all sit outside the box.
fn ring_meets_box(ring: &[(f32, f32)], min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
    if ring.is_empty() {
        return false;
    }
    // The cheap rejection first: rings and boxes are mostly far apart.
    let (mut lo_x, mut lo_y, mut hi_x, mut hi_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in ring {
        lo_x = lo_x.min(x as f64);
        lo_y = lo_y.min(y as f64);
        hi_x = hi_x.max(x as f64);
        hi_y = hi_y.max(y as f64);
        if x as f64 >= min_x && x as f64 <= max_x && y as f64 >= min_y && y as f64 <= max_y {
            return true;
        }
    }
    if hi_x < min_x || lo_x > max_x || hi_y < min_y || lo_y > max_y {
        return false;
    }
    // A box corner in the ring covers the box lying wholly inside the state.
    for (x, y) in [
        (min_x, min_y),
        (max_x, min_y),
        (max_x, max_y),
        (min_x, max_y),
    ] {
        if point_in_ring(x, y, ring) {
            return true;
        }
    }
    // Left over: an edge crosses the box without a vertex in it.
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let a = (ring[i].0 as f64, ring[i].1 as f64);
        let b = (ring[j].0 as f64, ring[j].1 as f64);
        j = i;
        if segment_meets_box(a, b, min_x, min_y, max_x, max_y) {
            return true;
        }
    }
    false
}

fn segment_meets_box(
    a: (f64, f64),
    b: (f64, f64),
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> bool {
    // Liang-Barsky: clip the segment against the box and see if anything is left.
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, a.0 - min_x),
        (dx, max_x - a.0),
        (-dy, a.1 - min_y),
        (dy, max_y - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return false;
            }
            t0 = t0.max(r);
        } else {
            if r < t0 {
                return false;
            }
            t1 = t1.min(r);
        }
    }
    t0 <= t1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_a_boundary() {
        for land in Land::ALL {
            let rings = &boundaries()[land as usize];
            assert!(!rings.is_empty(), "{} has no rings", land.code());
            assert!(rings.iter().all(|r| r.len() >= 4));
        }
    }

    #[test]
    fn places_land_in_the_right_state() {
        // Cologne, Hanover, Dresden, Munich, Mainz.
        assert_eq!(Land::at(50.938, 6.960), Some(Land::Nw));
        assert_eq!(Land::at(52.375, 9.732), Some(Land::Ni));
        assert_eq!(Land::at(51.050, 13.738), Some(Land::Sn));
        assert_eq!(Land::at(48.137, 11.575), Some(Land::By));
        assert_eq!(Land::at(49.992, 8.247), Some(Land::Rp));
    }

    #[test]
    fn a_longitude_lands_in_its_zone() {
        // Germany's two, and the neighbours a line could reach.
        assert_eq!(utm_zone_at(8.1), 32); // the Ruhr
        assert_eq!(utm_zone_at(13.4), 33); // Berlin
        assert_eq!(utm_zone_at(16.4), 33); // Vienna
        assert_eq!(utm_zone_at(4.9), 31); // Amsterdam
        assert_eq!(utm_zone_at(-1.0), 30); // west of Greenwich
        // The ends of the world stay in range rather than wrapping.
        assert_eq!(utm_zone_at(-180.0), 1);
        assert_eq!(utm_zone_at(180.0), 60);
    }

    #[test]
    fn the_north_sea_belongs_to_no_one() {
        assert_eq!(Land::at(54.5, 6.0), None);
    }

    #[test]
    fn a_box_on_a_border_asks_both_states() {
        // The Rhine at Emmerich: North Rhine-Westphalia and, 12 km north, the
        // Dutch border — the box has to find NW and nothing over the frontier.
        let found = Land::touching(51.80, 6.20, 51.90, 6.35);
        assert!(found.contains(&Land::Nw), "{found:?}");
        // Around Hann. Muenden, where Lower Saxony, Hesse and Thuringia meet.
        let found = Land::touching(51.35, 9.60, 51.50, 9.90);
        assert!(found.contains(&Land::Ni), "{found:?}");
        assert!(found.contains(&Land::He), "{found:?}");
    }

    #[test]
    fn a_box_inside_one_state_finds_it() {
        // A square kilometre in the Soester Boerde, far from any border.
        let found = Land::touching(51.565, 8.100, 51.575, 8.115);
        assert_eq!(found, vec![Land::Nw]);
    }

    #[test]
    fn zero_licence_needs_no_credit() {
        assert!(!Land::Sh.service().licence.needs_attribution());
        assert!(Land::Nw.service().licence.needs_attribution());
    }

    #[test]
    fn rhineland_palatinate_falls_back_to_openstreetmap() {
        let service = Land::Rp.service();
        assert_eq!(service.access, Access::Osm);
        assert_eq!(service.licence, Licence::Odbl);
        assert!(service.licence.needs_attribution());
    }

    #[test]
    fn the_gsa_states_are_fetchable() {
        for land in [Land::Nw, Land::Ni, Land::Bb, Land::Sn, Land::Th] {
            let service = land.service();
            assert_eq!(service.level, Level::Gsa, "{}", land.code());
            assert!(
                matches!(service.access, Access::Wfs | Access::WfsPointJoin),
                "{}",
                land.code()
            );
        }
    }
}
