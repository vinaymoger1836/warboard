//! Ground routes on the H3 terrain grid (doc 02 §5): A* over the cells of the
//! finest pack covering the start, with a speed multiplier per cell from the
//! caller (the realism profile's terrain factors).
//!
//! Costs are whole millimetres at full speed, so comparisons are exact and the
//! route is the same on every platform; ties go to the lower cell id.

use crate::{CellInfo, Geo, GeoError};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use wb_core::geo::{LatLon, distance_m};

/// A route over the grid.
#[derive(Clone, Debug, PartialEq)]
pub struct GroundPath {
    /// Centres of the cells after the start cell, ending with the exact
    /// destination.
    pub points: Vec<LatLon>,
    /// Metres along the route.
    pub length_m: f64,
    /// Seconds at a base speed of 1 m/s (divide by the formation's speed).
    pub cost_s_per_mps: f64,
    /// Cells the search opened.
    pub opened: u32,
}

/// Why no route was found; the message is shown to the player.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("No terrain data here: ground forces need a map pack that covers the area")]
    NoPack,
    #[error("The start is not on passable ground")]
    StartBlocked,
    #[error("The destination is not on passable ground")]
    DestBlocked,
    #[error("No route over land to the destination")]
    NoRoute,
    #[error("The destination is too far for one route; move in shorter legs")]
    TooFar,
    #[error("Cannot read the terrain: {0}")]
    Geo(#[from] GeoError),
}

impl Geo {
    /// Fastest route from `from` to `to`. `factor` gives a cell's speed
    /// multiplier (`None`: cannot be entered); `max_factor` is the largest
    /// value it can return (the heuristic's speed). The search stops after
    /// `max_cells` opened cells.
    pub fn ground_path(
        &self,
        from: LatLon,
        to: LatLon,
        max_factor: f64,
        max_cells: u32,
        factor: &dyn Fn(&CellInfo) -> Option<f64>,
    ) -> Result<GroundPath, PathError> {
        let pi = self.pack_at(from).ok_or(PathError::NoPack)?;
        let pk = &self.packs()[pi];
        let start = pk.cell(from)?;
        let goal = pk.cell(to)?;
        let speed_of = |cell: u64| -> Result<Option<f64>, PathError> {
            Ok(pk
                .terrain()
                .lookup(cell)?
                .and_then(|c| factor(&c))
                .filter(|f| f.is_finite() && *f > 0.0))
        };
        if speed_of(start)?.is_none() {
            return Err(PathError::StartBlocked);
        }
        if speed_of(goal)?.is_none() {
            return Err(PathError::DestBlocked);
        }
        let centre = |cell: u64| crate::h3::centre(cell).unwrap_or(from);
        if start == goal {
            let d = distance_m(from, to);
            let f = speed_of(start)?.unwrap_or(1.0);
            return Ok(GroundPath {
                points: vec![to],
                length_m: d,
                cost_s_per_mps: d / f,
                opened: 1,
            });
        }
        let goal_at = centre(goal);
        let mm = |d_m: f64, f: f64| (d_m * 1_000.0 / f).round().max(1.0) as u64;
        let h = |cell: u64| mm(distance_m(centre(cell), goal_at), max_factor.max(1e-6)) - 1;
        // cell -> (cost so far, parent, speed factor).
        let mut best: BTreeMap<u64, (u64, u64, f64)> = BTreeMap::new();
        let mut open: BinaryHeap<Reverse<(u64, u64, u64)>> = BinaryHeap::new();
        let f0 = speed_of(start)?.unwrap_or(1.0);
        best.insert(start, (0, start, f0));
        open.push(Reverse((h(start), 0, start)));
        let mut opened = 0u32;
        let mut found = false;
        while let Some(Reverse((_, g, cell))) = open.pop() {
            let Some(&(known, _, f_here)) = best.get(&cell) else {
                continue;
            };
            if g > known {
                continue;
            }
            if cell == goal {
                found = true;
                break;
            }
            opened += 1;
            if opened > max_cells {
                return Err(PathError::TooFar);
            }
            let here = centre(cell);
            for next in crate::h3::disk(cell, 1) {
                if next == cell {
                    continue;
                }
                let f_next = match best.get(&next) {
                    Some(b) => b.2,
                    None => match speed_of(next)? {
                        Some(f) => f,
                        None => continue,
                    },
                };
                let d = distance_m(here, centre(next));
                // Half the leg in each cell.
                let step = mm(d / 2.0, f_here) + mm(d / 2.0, f_next);
                let g2 = g + step;
                if best.get(&next).is_none_or(|b| g2 < b.0) {
                    best.insert(next, (g2, cell, f_next));
                    open.push(Reverse((g2 + h(next), g2, next)));
                }
            }
        }
        if !found {
            return Err(PathError::NoRoute);
        }
        let mut cells = vec![goal];
        let mut at = goal;
        while at != start {
            at = best[&at].1;
            cells.push(at);
        }
        cells.reverse();
        let mut points: Vec<LatLon> = cells[1..].iter().map(|c| centre(*c)).collect();
        if let Some(last) = points.last_mut() {
            *last = to;
        }
        let mut length = 0.0;
        let mut prev = from;
        for p in &points {
            length += distance_m(prev, *p);
            prev = *p;
        }
        Ok(GroundPath {
            points,
            length_m: length,
            cost_s_per_mps: best[&goal].0 as f64 / 1_000.0,
            opened,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TerrainClass;
    use crate::synthetic::{Patch, terrain_pack};

    fn geo(patches: &[Patch]) -> Geo {
        let mut g = Geo::new();
        g.add_pack(terrain_pack("t", 16, 55, 1, 1, 20, &[], patches).unwrap())
            .unwrap();
        g
    }

    fn factor(c: &CellInfo) -> Option<f64> {
        match c.class {
            TerrainClass::Open => Some(1.0),
            TerrainClass::Mountain => Some(0.3),
            TerrainClass::Urban => Some(0.5),
            _ => None,
        }
    }

    #[test]
    fn route_goes_round_water_and_slow_ground() {
        // Open land with a lake across the direct line, open at its north end.
        let g = geo(&[
            Patch::new(TerrainClass::Open, [16.1, 55.1, 16.9, 55.9]),
            Patch::new(TerrainClass::Water, [16.45, 55.1, 16.55, 55.7]),
        ]);
        let (a, b) = (LatLon::from_deg(55.3, 16.2), LatLon::from_deg(55.3, 16.8));
        let p = g.ground_path(a, b, 1.0, 100_000, &factor).unwrap();
        let direct = distance_m(a, b);
        assert!(p.length_m > direct * 1.5, "{} vs {direct}", p.length_m);
        for q in &p.points {
            assert_ne!(g.terrain_class(*q).unwrap(), TerrainClass::Water);
        }
        assert_eq!(*p.points.last().unwrap(), b);
        // The same search again gives the same route.
        assert_eq!(p, g.ground_path(a, b, 1.0, 100_000, &factor).unwrap());

        // A mountain belt with an open pass: the route takes the pass.
        let g = geo(&[
            Patch::new(TerrainClass::Open, [16.1, 55.1, 16.9, 55.9]),
            Patch::new(TerrainClass::Mountain, [16.4, 55.1, 16.6, 55.9]),
            Patch::new(TerrainClass::Open, [16.4, 55.6, 16.6, 55.65]),
        ]);
        let (a, b) = (LatLon::from_deg(55.5, 16.2), LatLon::from_deg(55.5, 16.8));
        let p = g.ground_path(a, b, 1.0, 100_000, &factor).unwrap();
        let mountain = p
            .points
            .iter()
            .filter(|q| g.terrain_class(**q).unwrap() == TerrainClass::Mountain)
            .count();
        assert!(mountain <= 2, "{mountain} mountain cells on the route");
    }

    #[test]
    fn no_route_across_the_sea() {
        let g = geo(&[
            Patch::new(TerrainClass::Open, [16.1, 55.1, 16.4, 55.9]),
            Patch::new(TerrainClass::Open, [16.6, 55.1, 16.9, 55.9]),
        ]);
        let (a, b) = (LatLon::from_deg(55.5, 16.2), LatLon::from_deg(55.5, 16.8));
        assert!(matches!(
            g.ground_path(a, b, 1.0, 100_000, &factor),
            Err(PathError::NoRoute)
        ));
        assert!(matches!(
            g.ground_path(a, LatLon::from_deg(55.5, 16.5), 1.0, 100_000, &factor),
            Err(PathError::DestBlocked)
        ));
    }
}
