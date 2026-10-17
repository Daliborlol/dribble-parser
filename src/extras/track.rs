//! Container builders of contract section 2.4: `TrackBuilder` (CSR change lists), `EventBuilder`
//! (columnar event lists) and `InstanceTable` (entity lifetimes, sorted by `(created, entityId)`).

use super::out::XNode;

pub const NONE_U32: u32 = u32::MAX;

/// Equality used for record de-duplication: floats compare bitwise, so NaN equals NaN.
pub trait BitEq {
    fn bit_eq(&self, other: &Self) -> bool;
}

macro_rules! biteq_int {
    ($($t:ty),*) => { $(impl BitEq for $t { #[inline] fn bit_eq(&self, o: &Self) -> bool { self == o } })* };
}
biteq_int!(u8, i8, u16, i16, u32, i32);

impl BitEq for f32 {
    #[inline]
    fn bit_eq(&self, o: &Self) -> bool {
        self.to_bits() == o.to_bits()
    }
}

/// A record is the full tuple of a track's (or event list's) fields.
pub trait Record: Copy {
    fn same(&self, other: &Self) -> bool;
    /// One typed-array column per field, named exactly as in `types.ts`.
    fn columns(rows: &[Self]) -> Vec<(String, XNode)>;
}

/// Defines a record struct. Field syntax: `rust_name: type => "jsName"`.
macro_rules! record {
    ($(#[$m:meta])* $name:ident { $($f:ident : $t:ident => $js:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name { $(pub $f: $t),* }
        impl $crate::extras::track::Record for $name {
            #[inline]
            fn same(&self, o: &Self) -> bool {
                true $(&& $crate::extras::track::BitEq::bit_eq(&self.$f, &o.$f))*
            }
            fn columns(rows: &[Self]) -> Vec<(String, $crate::extras::out::XNode)> {
                vec![$(
                    ($js.to_string(), <$t as $crate::extras::out::ColType>::col(rows.iter().map(|r| r.$f).collect()))
                ),*]
            }
        }
    };
}
pub(crate) use record;

/// CSR change list keyed by an integer key (section 2.4.1).
pub struct TrackBuilder<R: Record> {
    per_key: Vec<Vec<(u32, R)>>,
    ticks: u32,
}

impl<R: Record> TrackBuilder<R> {
    pub fn new(ticks: u32) -> Self {
        TrackBuilder {
            per_key: Vec::new(),
            ticks,
        }
    }

    /// `TrackBuilder::push` of section 2.4.
    pub fn push(&mut self, key: usize, tick: i64, rec: R) {
        if tick >= self.ticks as i64 {
            return;
        }
        let t = tick.max(0) as u32;
        if key >= self.per_key.len() {
            self.per_key.resize_with(key + 1, Vec::new);
        }
        let Some(list) = self.per_key.get_mut(key) else {
            return;
        };
        // ticks must stay strictly increasing per key even if the server tick ever goes back
        let t = match list.last() {
            Some(last) if t < last.0 => last.0,
            _ => t,
        };
        let len = list.len();
        let replace_same_tick = matches!(list.last(), Some(last) if last.0 == t);
        if replace_same_tick {
            if let Some(last) = list.last_mut() {
                last.1 = rec;
            }
            if len >= 2 {
                let prev_same = list
                    .get(len - 2)
                    .map(|prev| prev.1.same(&rec))
                    .unwrap_or(false);
                if prev_same {
                    list.pop();
                }
            }
        } else if matches!(list.last(), Some(last) if last.1.same(&rec)) {
            // unchanged
        } else {
            list.push((t, rec));
        }
    }

    pub fn last(&self, key: usize) -> Option<&R> {
        self.per_key.get(key).and_then(|l| l.last()).map(|(_, r)| r)
    }

    pub fn key_len(&self, key: usize) -> usize {
        self.per_key.get(key).map(|l| l.len()).unwrap_or(0)
    }

    pub fn total(&self) -> usize {
        self.per_key.iter().map(|l| l.len()).sum()
    }

    pub fn entries(&self, key: usize) -> &[(u32, R)] {
        self.per_key.get(key).map(|l| l.as_slice()).unwrap_or(&[])
    }

    /// CSR fields (`offsets`, `ticks`, one column per record field). `order[k]` is the old key
    /// that becomes key `k`; `None` keeps keys `0..key_count`.
    pub fn into_fields(self, key_count: usize, order: Option<&[usize]>) -> Vec<(String, XNode)> {
        let mut per_key = self.per_key;
        let mut offsets: Vec<u32> = Vec::with_capacity(key_count + 1);
        let total: usize = per_key.iter().map(|l| l.len()).sum();
        let mut ticks: Vec<u32> = Vec::with_capacity(total);
        let mut rows: Vec<R> = Vec::with_capacity(total);
        offsets.push(0);
        for k in 0..key_count {
            let old = match order {
                Some(o) => o.get(k).copied().unwrap_or(usize::MAX),
                None => k,
            };
            if let Some(list) = per_key.get_mut(old) {
                for (t, r) in list.drain(..) {
                    ticks.push(t);
                    rows.push(r);
                }
            }
            offsets.push(ticks.len() as u32);
        }
        drop(per_key);
        let mut fields = vec![
            ("offsets".to_string(), XNode::U32(offsets)),
            ("ticks".to_string(), XNode::U32(ticks)),
        ];
        fields.extend(R::columns(&rows));
        fields
    }

    /// Rewrites every record (used to map provisional instance ids to final ones).
    pub fn map_records(&mut self, f: impl Fn(&mut R)) {
        for list in self.per_key.iter_mut() {
            for (_, r) in list.iter_mut() {
                f(r);
            }
        }
    }
}

/// Empty track fields for an excluded group: `offsets` of length `key_count + 1`, all zero.
pub fn empty_track<R: Record>(key_count: usize) -> Vec<(String, XNode)> {
    let mut fields = vec![
        ("offsets".to_string(), XNode::U32(vec![0; key_count + 1])),
        ("ticks".to_string(), XNode::U32(Vec::new())),
    ];
    fields.extend(R::columns(&[]));
    fields
}

/// Columnar event list sorted by tick (stable, arrival order within a tick).
pub struct EventBuilder<R: Record> {
    rows: Vec<(u32, R)>,
    ticks: u32,
}

impl<R: Record> EventBuilder<R> {
    pub fn new(ticks: u32) -> Self {
        EventBuilder {
            rows: Vec::new(),
            ticks,
        }
    }

    /// Events before tick 0 or at `tick >= ticks` are dropped (section 1.2).
    pub fn push(&mut self, tick: i64, rec: R) {
        if tick < 0 || tick >= self.ticks as i64 {
            return;
        }
        self.rows.push((tick as u32, rec));
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn rows(&self) -> &[(u32, R)] {
        &self.rows
    }

    pub fn map_records(&mut self, f: impl Fn(&mut R)) {
        for (_, r) in self.rows.iter_mut() {
            f(r);
        }
    }

    pub fn into_fields(self) -> Vec<(String, XNode)> {
        let mut rows = self.rows;
        rows.sort_by_key(|(t, _)| *t);
        let ticks: Vec<u32> = rows.iter().map(|(t, _)| *t).collect();
        let recs: Vec<R> = rows.iter().map(|(_, r)| *r).collect();
        drop(rows);
        let mut fields = vec![("tick".to_string(), XNode::U32(ticks))];
        fields.extend(R::columns(&recs));
        fields
    }
}

/// One entity lifetime. Ticks are raw cache ticks (may be negative before the start).
#[derive(Debug, Clone)]
pub struct InstRow<T> {
    pub entity: u16,
    pub created: i64,
    pub deleted: Option<i64>,
    pub data: T,
}

pub struct InstanceTable<T> {
    pub rows: Vec<InstRow<T>>,
}

/// Result of sorting an instance table: `order[k]` = provisional index of final instance `k`,
/// `final_of[prov]` = final index or `NONE_U32` when dropped.
pub struct InstOrder {
    pub order: Vec<usize>,
    pub final_of: Vec<u32>,
}

impl InstOrder {
    pub fn map(&self, prov: u32) -> u32 {
        if prov == NONE_U32 {
            return NONE_U32;
        }
        self.final_of
            .get(prov as usize)
            .copied()
            .unwrap_or(NONE_U32)
    }
    pub fn map_i32(&self, prov: i32) -> i32 {
        if prov < 0 {
            return -1;
        }
        let f = self.map(prov as u32);
        if f == NONE_U32 {
            -1
        } else {
            f as i32
        }
    }
    pub fn map_u16(&self, prov: u16) -> u16 {
        if prov == u16::MAX {
            return u16::MAX;
        }
        let f = self.map(prov as u32);
        if f == NONE_U32 || f >= u16::MAX as u32 {
            u16::MAX
        } else {
            f as u16
        }
    }
    pub fn map_i16(&self, prov: i16) -> i16 {
        if prov < 0 {
            return -1;
        }
        let f = self.map(prov as u32);
        if f == NONE_U32 || f > i16::MAX as u32 {
            -1
        } else {
            f as i16
        }
    }
    pub fn count(&self) -> usize {
        self.order.len()
    }
}

impl<T> Default for InstanceTable<T> {
    fn default() -> Self {
        InstanceTable { rows: Vec::new() }
    }
}

impl<T> InstanceTable<T> {
    pub fn new() -> Self {
        InstanceTable { rows: Vec::new() }
    }

    /// Adds an instance and returns its provisional index.
    pub fn add(&mut self, entity: u32, created: i64, data: T) -> u32 {
        let i = self.rows.len() as u32;
        self.rows.push(InstRow {
            entity: entity.min(u16::MAX as u32) as u16,
            created,
            deleted: None,
            data,
        });
        i
    }

    pub fn get(&self, prov: u32) -> Option<&InstRow<T>> {
        self.rows.get(prov as usize)
    }

    pub fn get_mut(&mut self, prov: u32) -> Option<&mut InstRow<T>> {
        self.rows.get_mut(prov as usize)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Instances that end before tick 0 or start at or after `ticks` are dropped; the rest are
    /// sorted by `(created clamped to 0, entityId)`.
    pub fn order(&self, ticks: u32) -> InstOrder {
        self.order_filtered(ticks, |_| true)
    }

    pub fn order_filtered(&self, ticks: u32, keep: impl Fn(usize) -> bool) -> InstOrder {
        let mut kept: Vec<usize> = (0..self.rows.len())
            .filter(|i| {
                self.rows
                    .get(*i)
                    .map(|r| {
                        let ended_before = matches!(r.deleted, Some(d) if d < 0);
                        !ended_before && r.created < ticks as i64
                    })
                    .unwrap_or(false)
                    && keep(*i)
            })
            .collect();
        kept.sort_by_key(|i| {
            self.rows
                .get(*i)
                .map(|r| (r.created.max(0), r.entity))
                .unwrap_or((i64::MAX, u16::MAX))
        });
        let mut final_of = vec![NONE_U32; self.rows.len()];
        for (k, old) in kept.iter().enumerate() {
            if let Some(slot) = final_of.get_mut(*old) {
                *slot = k as u32;
            }
        }
        InstOrder {
            order: kept,
            final_of,
        }
    }

    /// `count`, `entityId`, `created`, `deleted` in final order.
    pub fn base_fields(&self, ord: &InstOrder, ticks: u32) -> Vec<(String, XNode)> {
        let mut entity = Vec::with_capacity(ord.count());
        let mut created = Vec::with_capacity(ord.count());
        let mut deleted = Vec::with_capacity(ord.count());
        for i in &ord.order {
            if let Some(r) = self.rows.get(*i) {
                entity.push(r.entity);
                created.push(r.created.max(0) as u32);
                deleted.push(match r.deleted {
                    Some(d) if d < ticks as i64 => d.max(0) as u32,
                    _ => NONE_U32,
                });
            }
        }
        vec![
            ("count".to_string(), XNode::Num(ord.count() as f64)),
            ("entityId".to_string(), XNode::U16(entity)),
            ("created".to_string(), XNode::U32(created)),
            ("deleted".to_string(), XNode::U32(deleted)),
        ]
    }

    /// One static column in final order.
    pub fn column<C: super::out::ColType>(
        &self,
        ord: &InstOrder,
        f: impl Fn(&InstRow<T>) -> C,
    ) -> XNode {
        let v: Vec<C> = ord
            .order
            .iter()
            .filter_map(|i| self.rows.get(*i).map(&f))
            .collect();
        C::col(v)
    }
}

/// Instance fields of an excluded group.
pub fn empty_instances() -> Vec<(String, XNode)> {
    vec![
        ("count".to_string(), XNode::Num(0.0)),
        ("entityId".to_string(), XNode::U16(Vec::new())),
        ("created".to_string(), XNode::U32(Vec::new())),
        ("deleted".to_string(), XNode::U32(Vec::new())),
    ]
}
