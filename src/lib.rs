#![no_std]

extern crate smallvec;
use smallvec::SmallVec;

pub struct SyncDevice {
    /// sync tracks (the vertical columns in the editor)
    pub tracks: SmallVec<[SyncTrack; 64]>,
    /// rows per beat
    pub rpb: u8,
    /// beats per minute
    pub bpm: f64,
    /// rows per second
    pub rps: f64,
    pub is_paused: bool,
    /// current row
    pub row: u32,
    /// current time in milliseconds
    pub time: u32,
}

impl SyncDevice {
    pub fn new(bpm: f64, rpb: u8) -> SyncDevice {
        SyncDevice {
            tracks: SmallVec::new(),
            rpb: rpb,
            bpm: bpm,
            rps: rps(bpm, rpb),
            is_paused: true,
            row: 0,
            time: 0,
        }
    }

    pub fn set_row_from_time(&mut self) {
        let r: f64 = (self.time as f64 / 1000.0) * self.rps + 0.5;
        self.row = r as u32;
    }

    pub fn get_track_value(&self, track_id: usize) -> Result<f64, SyncError> {
        if self.tracks.len() > track_id {
            return Ok(self.tracks[track_id].value_at(self.row));
        } else {
            return Err(SyncError::TrackDoesntExist);
        }
    }

    pub fn get_track_value_smooth(&self, track_id: usize) -> Result<f64, SyncError> {
        if self.tracks.len() > track_id {
            let track = &self.tracks[track_id];
            let row = self.get_row_from_time();

            let hit_idx: usize;
            if let Some(idx) = track.find_active_key_idx_for_row(row) {
                use self::ActiveKeyIdx::*;
                match idx {
                    ActiveKeyIdx::ExactRow(n) | ActiveKeyIdx::PrevRow(n) => {
                        if n == track.keys.len() - 1 {
                            return Ok(track.keys[track.keys.len() - 1].value as f64);
                        }
                        hit_idx = n
                    },
                    AfterLastRow => return Ok(track.keys[track.keys.len() - 1].value as f64),
                    BeforeFirstRow => return Ok(track.keys[0].value as f64),
                }
            } else {
                return Ok(0.0);
            }

            let cur_key = &track.keys[hit_idx];
            // Return early for Step keys
            match cur_key.key_type {
                KeyType::Step => return Ok(cur_key.value as f64),
                _ => {}
            }

            // Calculate interpolated keys
            let next_key = &track.keys[hit_idx + 1];
            let cur_key_time = self.get_time_from_key(cur_key) as f64;
            let next_key_time = self.get_time_from_key(next_key) as f64;
            let t = (self.time as f64 - cur_key_time) / (next_key_time - cur_key_time);
            let a = cur_key.value as f64;
            let b = (next_key.value - cur_key.value) as f64;

            Ok(match cur_key.key_type {
                KeyType::Linear => a + b * t,
                KeyType::Smooth => a + b * (t * t * (3.0 - 2.0 * t)),
                KeyType::Ramp => a + b * t * t,
                _ => 0.0,
            })
        } else {
            Err(SyncError::TrackDoesntExist)
        }
    }

    fn get_row_from_time(&self) -> u32 {
        ((self.time as f64 / 1000.0) * self.rps) as u32
    }

    fn get_time_from_key(&self, key: &TrackKey) -> u32 {
        (key.row as f64 / self.rps * 1000.0) as u32
    }
}

pub struct SyncTrack {
    /// key frames, rows where values change
    pub keys: SmallVec<[TrackKey; 64]>,
}

pub enum SyncError {
    TrackDoesntExist
}

pub struct TrackKey {
    pub row: u32,
    pub value: f32,
    /// interpolation type
    pub key_type: KeyType,
}

pub enum KeyType {
    Step, // constant until value changes
    Linear, // linear interpolation
    Smooth, // smooth curve
    Ramp, // exponential ramp
    NOOP,
}

pub enum ActiveKeyIdx {
    /// key is on this row
    ExactRow(usize),
    /// key is on a previous row
    PrevRow(usize),
    /// the row is before the first key
    BeforeFirstRow,
    /// row moved past the last row
    AfterLastRow,
}

impl SyncTrack {
    pub fn new() -> SyncTrack {
        SyncTrack {
            keys: SmallVec::new(),
        }
    }

    /// Adds a key to the track, inserting sorted by row, replacing if one already exists on that row
    pub fn add_key(&mut self, track_key: TrackKey) {

        let res = self.find_active_key_idx_for_row(track_key.row);

        if let Some(idx) = res {
            // Some kind of active key
            use self::ActiveKeyIdx::*;
            match idx {
                // replace key
                ExactRow(n) => self.keys[n] = track_key,

                // add new key
                PrevRow(n) => self.keys.insert(n+1, track_key),
                BeforeFirstRow => self.keys.insert(0, track_key),
                AfterLastRow => self.keys.push(track_key),
            }
        } else {
            // No keys, first key
            self.keys.push(track_key);
        }
    }

    /// Deletes the key found on the given row
    pub fn delete_key(&mut self, row: u32) {
        if let Some(idx) = self.find_key_idx_by_row(row) {
            self.keys.remove(idx);
        }
    }

    /// Returns index of the key with the given row, or `None`
    pub fn find_key_idx_by_row(&self, row: u32) -> Option<usize> {
        for (idx, key) in self.keys.iter().enumerate() {
            if key.row == row {
                return Some(idx);
            }
        }

        None
    }

    pub fn value_at(&self, row: u32) -> f64 {

        let hit_idx: usize;

        if let Some(hit) = self.find_active_key_idx_for_row(row) {
            use self::ActiveKeyIdx::*;
            match hit {
                ExactRow(n) => return self.keys[n].value as f64,

                PrevRow(n) => hit_idx = n,

                // hit is beyond the last key
                AfterLastRow => return self.keys[self.keys.len() - 1].value as f64,

                BeforeFirstRow => return self.keys[0].value as f64,
            }
        } else {
            return 0.0;
        }

        // return interpolated value
        let cur_key = &self.keys[hit_idx];
        let next_key = &self.keys[hit_idx + 1];

	      let t: f64 = ((row - cur_key.row) as f64) / ((next_key.row - cur_key.row) as f64);
        let a: f64 = cur_key.value as f64;
        let b: f64 = (next_key.value - cur_key.value) as f64;

        use self::KeyType::*;
        match cur_key.key_type {
            Step => return a,

            Linear => return a + b * t,

            Smooth => return a + b * (t*t * (3.0 - 2.0 * t)),

            Ramp => return a + b * t*t,

            NOOP => return 0.0,
        }

    }

    /// Find the active key idx for a row
    pub fn find_active_key_idx_for_row(&self, row: u32) -> Option<ActiveKeyIdx> {
        if self.keys.len() == 0 {
            return None;
        }

        // Linear search. Keys are sorted by row.

        let mut hit_idx: usize = 0;
        let mut ret: Option<ActiveKeyIdx> = None;

        for (idx, key) in self.keys.iter().enumerate() {
            if key.row == row {
                return Some(ActiveKeyIdx::ExactRow(idx));
            } else if key.row < row {
                hit_idx = idx;
                ret = Some(ActiveKeyIdx::PrevRow(hit_idx));
            }
        }

        if hit_idx == self.keys.len() - 1 {
            return Some(ActiveKeyIdx::AfterLastRow);
        }

        if hit_idx == 0 && ret.is_none() {
            return Some(ActiveKeyIdx::BeforeFirstRow);
        }

        ret
    }
}

impl TrackKey {
    pub fn new() -> TrackKey {
        TrackKey {
            row: 0,
            value: 0.0,
            key_type: KeyType::Step,
        }
    }
}

/// Calculate rows per second
pub fn rps(bpm: f64, rpb: u8) -> f64 {
    (bpm / 60.0) * (rpb as f64)
}

pub fn key_to_code(key: &KeyType) -> u8 {
    use self::KeyType::*;
    match *key {
        Step      => 0,
        Linear    => 1,
        Smooth    => 2,
        Ramp      => 3,
        NOOP      => 255,
    }
}

pub fn code_to_key(code: u8) -> KeyType {
    use self::KeyType::*;
    match code {
        0 => Step,
        1 => Linear,
        2 => Smooth,
        3 => Ramp,
        _ => NOOP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_track(interpolation: u8) -> SyncTrack {
        let mut track = SyncTrack::new();

        track.add_key(TrackKey {
            row: 0,
            value: 0f32,
            key_type: code_to_key(interpolation),
        });

        track.add_key(TrackKey {
            row: 3,
            value: 1f32,
            key_type: code_to_key(interpolation),
        });

        track
    }

    #[test]
    fn test_can_get_track_value() {
        let mut device = SyncDevice::new(120.0, 4);
        device
            .tracks
            .push(setup_track(key_to_code(&KeyType::Linear)));

        device.row = 0;
        assert_eq!(0.0, device.get_track_value(0).unwrap_or(-1.0));

        device.row = 1;
        assert_eq!(
            0.3333333333333333,
            device.get_track_value(0).unwrap_or(-1.0)
        );

        device.row = 2;
        assert_eq!(
            0.6666666666666666,
            device.get_track_value(0).unwrap_or(-1.0)
        );

        device.row = 3;
        assert_eq!(1.0, device.get_track_value(0).unwrap_or(-1.0));
    }

    #[test]
    fn test_can_get_track_value_smooth() {
        let mut device = SyncDevice::new(120.0, 4);
        device
            .tracks
            .push(setup_track(key_to_code(&KeyType::Linear)));

        // At 120 BPM we have 0.5s per beat, and 4 rows per beat gives 0.125s per row.
        // So the values at 0, 0.125, 0.25 and 0.375 should line up with the values for the rows.
        device.time = 0;
        assert_eq!(
            0.0,
            device
                .get_track_value_smooth(0)
                .unwrap_or(-1.0)
        );
        device.time = 125;
        assert_eq!(
            0.3333333333333333,
            device
                .get_track_value_smooth(0)
                .unwrap_or(-1.0)
        );
        device.time = 250;
        assert_eq!(
            0.6666666666666666,
            device
                .get_track_value_smooth(0)
                .unwrap_or(-1.0)
        );
        device.time = 375;
        assert_eq!(
            1.0,
            device
                .get_track_value_smooth(0)
                .unwrap_or(-1.0)
        );

        // But now we should be able to interpolate between rows as well..
        device.time = 63;
        assert_eq!(
            0.168,
            device
                .get_track_value_smooth(0)
                .unwrap_or(-1.0)
        );
        device.time = 188;
        assert_eq!(
            0.5013333333333333,
            device
                .get_track_value_smooth(0)
                .unwrap_or(-1.0)
        );
        device.time = 312;
        assert_eq!(
            0.832,
            device
                .get_track_value_smooth(0)
                .unwrap_or(-1.0)
        );
    }
}