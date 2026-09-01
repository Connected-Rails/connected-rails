//! The imagery as one continuous picture, fetched a tile at a time.
//!
//! A model wants a square of pixels; the imagery comes in 256-pixel tiles on
//! a Web Mercator grid. The obvious bridge — stitch the whole region into one
//! bitmap and cut windows out of it — falls over at the second module: ten
//! kilometres square at thirty centimetres a pixel is a gigapixel, and three
//! gigabytes of it.
//!
//! So the sheet is virtual. It is the whole world at one zoom level, addressed
//! in global pixels, and it holds only the tiles that the windows asked for,
//! newest first, up to a cap. A run along a corridor touches a ribbon of tiles
//! and never allocates the square it would have fitted in.
//!
//! Tiles come from a callback rather than from [`imagery::ImagerySource`]
//! directly: the editor's overlay already owns a source, and a background
//! import must not reach into a Bevy resource from its own thread. The editor
//! hands in a closure that goes to the same disk cache and the same provider —
//! so a region the user has already looked at is read from disk and the run
//! starts instantly.

use imagery::{DecodedTile, TileId, tiles};
use std::collections::HashMap;

/// One window of imagery, RGB8, row by row from the top.
pub struct Window {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// Tiles that could not be had — their area is left black. A window that
    /// is mostly missing is not worth inferring, and [`Window::coverage`] is
    /// what says so.
    pub missing: usize,
    pub tiles: usize,
}

impl Window {
    /// Share of the window that actually carries imagery, 0 … 1.
    pub fn coverage(&self) -> f64 {
        if self.tiles == 0 {
            return 0.0;
        }
        1.0 - self.missing as f64 / self.tiles as f64
    }
}

/// The world at one zoom level, tiles held on demand.
pub struct Sheet {
    pub zoom: u8,
    /// Edge length of a provider tile [px].
    pub tile_size: u32,
    /// `None` marks a tile that was asked for and could not be had, so it is
    /// not asked for again — a provider with a hole in its coverage would
    /// otherwise be re-queried once per overlapping window.
    tiles: HashMap<TileId, Option<DecodedTile>>,
    /// Least-recently-used order, oldest first.
    order: Vec<TileId>,
    limit: usize,
    fetch: Box<dyn FnMut(TileId) -> Option<DecodedTile> + Send>,
    /// How many tiles have been asked of the callback — what the progress
    /// display counts.
    pub requested: usize,
}

impl Sheet {
    /// `limit` is how many tiles stay in memory. Two rows of windows is
    /// plenty: the walk is row by row, so a tile is wanted again by the
    /// window to the right and by the one below, and never after that.
    pub fn new(
        zoom: u8,
        tile_size: u32,
        limit: usize,
        fetch: impl FnMut(TileId) -> Option<DecodedTile> + Send + 'static,
    ) -> Self {
        Self {
            zoom,
            tile_size: tile_size.max(1),
            tiles: HashMap::new(),
            order: Vec::new(),
            limit: limit.max(4),
            fetch: Box::new(fetch),
            requested: 0,
        }
    }

    /// Global pixel position of a place in degrees.
    pub fn pixel_of(&self, lat: f64, lon: f64) -> (f64, f64) {
        let (x, y) = tiles::world_xy(lat, lon, self.zoom);
        (x * self.tile_size as f64, y * self.tile_size as f64)
    }

    /// The place a global pixel position sits at, in degrees.
    pub fn lat_lon_at(&self, px: f64, py: f64) -> (f64, f64) {
        tiles::lat_lon_at(
            px / self.tile_size as f64,
            py / self.tile_size as f64,
            self.zoom,
        )
    }

    /// Ground resolution at a latitude [m/px]. Mercator stretches with the
    /// latitude, so this is the only honest way to turn pixels into metres.
    pub fn meters_per_pixel(&self, lat: f64) -> f64 {
        tiles::EARTH_CIRCUMFERENCE * lat.to_radians().cos()
            / (self.tile_size as f64 * TileId::count(self.zoom) as f64)
    }

    /// Reads a window out of the sheet, fetching whatever it stands on.
    pub fn window(&mut self, left: i64, top: i64, width: u32, height: u32) -> Window {
        let size = self.tile_size as i64;
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 3];
        let count = TileId::count(self.zoom) as i64;
        let first_x = left.div_euclid(size);
        let last_x = (left + width as i64 - 1).div_euclid(size);
        let first_y = top.div_euclid(size);
        let last_y = (top + height as i64 - 1).div_euclid(size);

        let (mut asked, mut missing) = (0, 0);
        for ty in first_y..=last_y {
            for tx in first_x..=last_x {
                asked += 1;
                // Outside the grid there is nothing to fetch — the poles and
                // the seam at the date line.
                if ty < 0 || ty >= count {
                    missing += 1;
                    continue;
                }
                let id = TileId::new(self.zoom, tx.rem_euclid(count) as u32, ty as u32);
                let Some(tile) = self.tile(id) else {
                    missing += 1;
                    continue;
                };
                blit(
                    tile,
                    tx * size - left,
                    ty * size - top,
                    size,
                    &mut pixels,
                    width,
                    height,
                );
            }
        }
        Window {
            width,
            height,
            pixels,
            missing,
            tiles: asked,
        }
    }

    /// A tile from memory, from the callback, or not at all.
    fn tile(&mut self, id: TileId) -> Option<&DecodedTile> {
        if !self.tiles.contains_key(&id) {
            self.requested += 1;
            let tile = (self.fetch)(id);
            self.tiles.insert(id, tile);
            self.order.push(id);
            while self.order.len() > self.limit {
                let oldest = self.order.remove(0);
                self.tiles.remove(&oldest);
            }
        } else if let Some(at) = self.order.iter().position(|&t| t == id) {
            // Touch it: the walk comes back to the tile under the overlap, and
            // dropping that one is what would double the downloads.
            let id = self.order.remove(at);
            self.order.push(id);
        }
        self.tiles.get(&id).and_then(|t| t.as_ref())
    }
}

/// Copies a tile into the window at `(offset_x, offset_y)`, clipped.
///
/// `nominal` is the tile size the sheet's geometry assumes; a provider that
/// answers with a different one (a 512-pixel retina tile for a 256-pixel
/// grid) is sampled to it rather than refused, so the picture is right even
/// when the configuration is not.
fn blit(
    tile: &DecodedTile,
    offset_x: i64,
    offset_y: i64,
    nominal: i64,
    out: &mut [u8],
    width: u32,
    height: u32,
) {
    let (tw, th) = (tile.width.max(1), tile.height.max(1));
    let from_x = offset_x.max(0);
    let to_x = (offset_x + nominal).min(width as i64);
    let from_y = offset_y.max(0);
    let to_y = (offset_y + nominal).min(height as i64);
    for y in from_y..to_y {
        let sy = ((y - offset_y) * th as i64 / nominal).clamp(0, th as i64 - 1) as u32;
        for x in from_x..to_x {
            let sx = ((x - offset_x) * tw as i64 / nominal).clamp(0, tw as i64 - 1) as u32;
            let source = ((sy * tw + sx) * 4) as usize;
            let target = ((y as u32 * width + x as u32) * 3) as usize;
            if let (Some(rgba), Some(rgb)) = (
                tile.pixels.get(source..source + 3),
                out.get_mut(target..target + 3),
            ) {
                rgb.copy_from_slice(rgba);
            }
        }
    }
}

/// The zoom level to fetch a model's imagery at.
///
/// Whatever level is used, the window is resampled so that a car comes out
/// the number of pixels the model expects — the scale is right either way.
/// What differs is sharpness, and that is decided the same way a photograph
/// is: **never enlarge.** So this takes the coarsest level that is still
/// finer than the model asks for, and the picture is reduced onto the input
/// rather than blown up to it.
///
/// The floor at 0.55 is what stops a level from being taken that is nearly
/// twice as fine as needed — each level is four times the tiles, and past
/// that point they buy detail the model was never trained to use.
pub fn zoom_for(ground_sample: f64, lat: f64, tile_size: u32, min: u8, max: u8) -> u8 {
    let resolution = |z: u8| {
        tiles::EARTH_CIRCUMFERENCE * lat.to_radians().cos()
            / (tile_size.max(1) as f64 * TileId::count(z) as f64)
    };
    let wanted = ground_sample.max(1e-6) * 0.55;
    let mut best = min;
    for z in min..=max {
        if resolution(z) >= wanted {
            best = z;
        }
    }
    // Nothing fine enough — the provider stops before the model would like.
    // Its finest is then the best there is.
    if resolution(best) > ground_sample * 1.5 && best < max {
        return max;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tile of one flat colour, with its own id written into the first pixel
    /// so a blit can be traced back to the tile it came from.
    fn tile(id: TileId, colour: [u8; 3]) -> DecodedTile {
        let size = 256;
        let mut pixels = vec![0u8; (size * size * 4) as usize];
        for p in pixels.as_chunks_mut::<4>().0 {
            p[0] = colour[0];
            p[1] = colour[1];
            p[2] = colour[2];
            p[3] = 255;
        }
        DecodedTile {
            tile: id,
            width: size,
            height: size,
            pixels,
        }
    }

    fn sheet() -> Sheet {
        Sheet::new(18, 256, 16, |id| {
            // Colour by parity, so neighbouring tiles differ.
            let shade = if (id.x + id.y) % 2 == 0 { 40 } else { 200 };
            Some(tile(id, [shade, shade, shade]))
        })
    }

    #[test]
    fn a_window_inside_one_tile_is_that_tiles_colour() {
        let mut sheet = sheet();
        // Tile (18, 0, 0) covers pixels 0…255.
        let window = sheet.window(10, 10, 64, 64);
        assert_eq!(window.missing, 0);
        assert_eq!(window.coverage(), 1.0);
        assert_eq!(&window.pixels[..3], &[40, 40, 40]);
        assert_eq!(sheet.requested, 1);
    }

    #[test]
    fn a_window_over_a_seam_carries_both_tiles() {
        let mut sheet = sheet();
        // Straddling the boundary at x = 256.
        let window = sheet.window(224, 0, 64, 8);
        assert_eq!(&window.pixels[..3], &[40, 40, 40], "left of the seam");
        // Column 40 of the first row: past the seam at column 32.
        let right = 40 * 3;
        assert_eq!(
            &window.pixels[right..right + 3],
            &[200, 200, 200],
            "right of the seam"
        );
        assert_eq!(sheet.requested, 2);
    }

    #[test]
    fn a_tile_that_cannot_be_had_is_asked_for_once() {
        let mut asked = 0;
        let mut sheet = Sheet::new(18, 256, 16, move |_| {
            asked += 1;
            assert!(asked <= 1, "a hole in the coverage must not be re-queried");
            None
        });
        let first = sheet.window(0, 0, 32, 32);
        assert_eq!(first.missing, 1);
        assert_eq!(first.coverage(), 0.0);
        assert!(first.pixels.iter().all(|&b| b == 0));
        sheet.window(4, 4, 32, 32);
        assert_eq!(sheet.requested, 1);
    }

    #[test]
    fn the_cache_holds_only_its_limit() {
        let mut sheet = Sheet::new(18, 256, 4, |id| Some(tile(id, [1, 2, 3])));
        for x in 0..10 {
            sheet.window(x * 256, 0, 16, 16);
        }
        assert_eq!(sheet.requested, 10);
        assert!(sheet.tiles.len() <= 4, "{}", sheet.tiles.len());
    }

    #[test]
    fn pixels_and_degrees_are_inverse() {
        let sheet = sheet();
        let (lat, lon) = (51.2277, 6.7735);
        let (px, py) = sheet.pixel_of(lat, lon);
        let (back_lat, back_lon) = sheet.lat_lon_at(px, py);
        assert!((back_lat - lat).abs() < 1e-9, "{back_lat}");
        assert!((back_lon - lon).abs() < 1e-9, "{back_lon}");
    }

    #[test]
    fn the_resolution_is_the_familiar_web_mercator_figure() {
        // Zoom 19 at 51° is a shade over 20 cm a pixel — the number every
        // slippy-map table gives.
        let sheet = Sheet::new(19, 256, 4, |_| None);
        let metres = sheet.meters_per_pixel(51.0);
        assert!((0.18..0.22).contains(&metres), "{metres}");
    }

    #[test]
    fn the_zoom_is_the_coarsest_one_that_still_beats_the_model() {
        // 30 cm at 51° sits between zoom 18 (0.37 m) and zoom 19 (0.19 m).
        // Zoom 18 would have to be enlarged onto the input, so 19 it is.
        assert_eq!(zoom_for(0.3, 51.0, 256, 1, 22), 19);
        // Twice as coarse a model is one level down.
        assert_eq!(zoom_for(0.6, 51.0, 256, 1, 22), 18);
        assert_eq!(zoom_for(0.15, 51.0, 256, 1, 22), 20);
        // A ceiling the provider imposes is respected.
        assert_eq!(zoom_for(0.15, 51.0, 256, 1, 18), 18);
    }
}
