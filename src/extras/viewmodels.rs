//! `players.vm0` / `players.vm1` from `CTFViewModel` (section 2.5.1).

use super::players::{VmRec, D_VM0, D_VM1};
use super::tracker::{ClassInfo, Ent, Role, Val};
use super::{to_u8, Ex, Slots};

#[derive(Default, Clone, Copy)]
pub struct VmSlots {
    pub model: Option<u32>,
    pub seq: Option<u32>,
    pub index: Option<u32>,
    pub rate: Option<u32>,
    pub effects: Option<u32>,
    pub parity: Option<u32>,
    pub weapon: Option<u32>,
    pub owner: Option<u32>,
    pub muzzle: Option<u32>,
}

pub fn setup(ci: &mut ClassInfo) -> Vec<String> {
    let t = "DT_BaseViewModel";
    let s = VmSlots {
        model: ci.find(t, "m_nModelIndex"),
        seq: ci.find(t, "m_nSequence"),
        index: ci.find(t, "m_nViewModelIndex"),
        rate: ci.find(t, "m_flPlaybackRate"),
        effects: ci.find(t, "m_fEffects"),
        parity: ci.find(t, "m_nAnimationParity"),
        weapon: ci.find(t, "m_hWeapon"),
        owner: ci.find(t, "m_hOwner"),
        muzzle: ci.find(t, "m_nMuzzleFlashParity"),
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("m_nModelIndex", s.model),
        ("m_nSequence", s.seq),
        ("m_nViewModelIndex", s.index),
        ("m_hWeapon", s.weapon),
        ("m_hOwner", s.owner),
        ("m_nAnimationParity", s.parity),
        ("m_nMuzzleFlashParity", s.muzzle),
    ] {
        if x.is_none() {
            missing.push(format!("{t}.{n}"));
        }
    }
    for x in [
        s.model, s.seq, s.index, s.rate, s.effects, s.parity, s.weapon, s.owner, s.muzzle,
    ] {
        ci.add_dep(x, 1);
    }
    ci.keep_only(&[
        s.model, s.seq, s.index, s.rate, s.effects, s.parity, s.weapon, s.owner, s.muzzle,
    ]);
    ci.slots = Slots::ViewModel(s);
    missing
}

impl Ex {
    fn vm_slots(&self, ent: &Ent) -> Option<VmSlots> {
        let ci = self.class_info(ent.class)?;
        match &ci.slots {
            Slots::ViewModel(s) => Some(*s),
            _ => None,
        }
    }

    pub(crate) fn vm_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, _st: i64) {
        let Slots::ViewModel(s) = &ci.slots else {
            return;
        };
        let p = self.p_of_handle(ent.h(s.owner));
        let which = (ent.i0(s.index) & 1) as usize;
        ent.role = Role::ViewModel;
        ent.slot = (p as u32) | ((which as u32) << 8);
        if p == 255 {
            return;
        }
        if let Some(slots) = self.pl.vm_ent.get_mut(p as usize) {
            if let Some(cell) = slots.get_mut(which) {
                *cell = Some(idx);
            }
        }
        self.pl
            .mark(p as usize, if which == 0 { D_VM0 } else { D_VM1 });
    }

    pub(crate) fn vm_update(
        &mut self,
        idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        _changed: &[(u32, Val)],
        st: i64,
    ) {
        // the owner and index are static; re-register in case they arrive late
        let Slots::ViewModel(s) = &ci.slots else {
            return;
        };
        let p = self.p_of_handle(ent.h(s.owner));
        let which = (ent.i0(s.index) & 1) as usize;
        let slot = (p as u32) | ((which as u32) << 8);
        if slot != ent.slot {
            ent.slot = slot;
            if p != 255 {
                if let Some(cell) = self
                    .pl
                    .vm_ent
                    .get_mut(p as usize)
                    .and_then(|s| s.get_mut(which))
                {
                    *cell = Some(idx);
                }
            }
        }
        let _ = st;
        self.vm_dirty(ent);
    }

    pub(crate) fn vm_dirty(&mut self, ent: &Ent) {
        let p = (ent.slot & 0xFF) as usize;
        let which = (ent.slot >> 8) & 1;
        if p != 255 {
            self.pl.mark(p, if which == 0 { D_VM0 } else { D_VM1 });
        }
    }

    pub(crate) fn vm_end(&mut self, idx: u32, ent: &Ent) {
        let p = (ent.slot & 0xFF) as usize;
        let which = ((ent.slot >> 8) & 1) as usize;
        if let Some(cell) = self.pl.vm_ent.get_mut(p).and_then(|s| s.get_mut(which)) {
            if *cell == Some(idx) {
                *cell = None;
            }
        }
        self.vm_dirty(ent);
    }

    /// Viewmodel record of player p, viewmodel `which` (nodraw when absent or dormant).
    pub(crate) fn vm_record(&self, p: usize, which: usize) -> VmRec {
        let none = VmRec {
            model: 0,
            seq: 0,
            parity: 0,
            muzzle: 0,
            weapon_def: 65535,
            rate: 0.0,
            nodraw: 1,
        };
        let Some(idx) = self
            .pl
            .vm_ent
            .get(p)
            .and_then(|s| s.get(which))
            .copied()
            .flatten()
        else {
            return none;
        };
        let Some(ent) = self.ents.get(idx as usize).and_then(|e| e.as_ref()) else {
            return none;
        };
        if ent.role != Role::ViewModel {
            return none;
        }
        let Some(s) = self.vm_slots(ent) else {
            return none;
        };
        let weapon_def = self
            .slot_of_handle(ent.h(s.weapon), Role::Weapon)
            .and_then(|w| self.wp.inst.get(w))
            .map(|r| r.data.def)
            .unwrap_or(65535);
        let effects = ent.i0(s.effects);
        VmRec {
            model: ent.i0(s.model).clamp(0, 65535) as u16,
            seq: to_u8(ent.i0(s.seq)),
            parity: to_u8(ent.i0(s.parity)),
            muzzle: to_u8(ent.i0(s.muzzle)),
            weapon_def,
            rate: ent.f0(s.rate),
            nodraw: ((effects & 32) != 0 || ent.dormant) as u8,
        }
    }
}
