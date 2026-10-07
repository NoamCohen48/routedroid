//! Picking the LAN interface from the daemon's list.

use routedroid_ipc::InterfaceInfo;

use super::{Field, LineInput, StartForm};

impl StartForm {
    /// New interface list: keep the pick if it is still there, else take the
    /// one with the default route.
    pub fn offer(&mut self, interfaces: &[InterfaceInfo]) {
        self.choices = interfaces
            .iter()
            .filter(|i| i.ineligible.is_none())
            .cloned()
            .collect();
        let kept = self.choices.iter().any(|i| i.name == self.lan_if.value());
        if !kept {
            let best = self
                .choices
                .iter()
                .find(|i| i.default_route)
                .or(self.choices.first());
            if let Some(best) = best {
                self.lan_if = LineInput::new(&best.name);
            }
        }
    }

    /// The blocks the helper allows phones to take on the picked interface.
    pub fn phone_hint(&self) -> Option<String> {
        let picked = self
            .choices
            .iter()
            .find(|i| i.name == self.lan_if.value())?;
        let blocks: Vec<String> = picked
            .phone_addresses
            .iter()
            .map(|net| format!("{}/{}", net.address, net.prefix))
            .collect();
        (!blocks.is_empty()).then(|| format!("allowed: {}", blocks.join(" ")))
    }

    pub fn picking(&self) -> bool {
        self.focused == Field::LanIf && !self.choices.is_empty()
    }

    /// Pick the next interface, or the previous one, wrapping around.
    pub fn pick(&mut self, forward: bool) {
        let count = self.choices.len();
        if count == 0 {
            return;
        }
        let at = self
            .choices
            .iter()
            .position(|i| i.name == self.lan_if.value());
        let next = match at {
            None => 0,
            Some(at) if forward => (at + 1) % count,
            Some(at) => (at + count - 1) % count,
        };
        self.lan_if = LineInput::new(&self.choices[next].name);
    }
}
