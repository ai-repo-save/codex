pub(super) enum SudoPromptAction {
    Output(Vec<u8>),
    Prompt,
}

pub(super) struct SudoPromptFilter {
    sentinel: Vec<u8>,
    pending: Vec<u8>,
}

impl SudoPromptFilter {
    pub(super) fn new(sentinel: String) -> Self {
        Self {
            sentinel: sentinel.into_bytes(),
            pending: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, chunk: Vec<u8>) -> Vec<SudoPromptAction> {
        self.pending.extend(chunk);
        let mut actions = Vec::new();
        while let Some(position) = self
            .pending
            .windows(self.sentinel.len())
            .position(|window| window == self.sentinel)
        {
            if position > 0 {
                actions.push(SudoPromptAction::Output(
                    self.pending.drain(..position).collect(),
                ));
            }
            self.pending.drain(..self.sentinel.len());
            actions.push(SudoPromptAction::Prompt);
        }

        let retained = self.sentinel.len().saturating_sub(1);
        if self.pending.len() > retained {
            let emitted = self.pending.len() - retained;
            actions.push(SudoPromptAction::Output(
                self.pending.drain(..emitted).collect(),
            ));
        }
        actions
    }

    pub(super) fn finish(mut self) -> Option<Vec<u8>> {
        (!self.pending.is_empty()).then_some(std::mem::take(&mut self.pending))
    }
}
