pub(super) enum SudoPromptAction {
    Output(Vec<u8>),
    Prompt,
    Started,
}

pub(super) struct SudoPromptFilter {
    prompt_sentinel: Vec<u8>,
    started_sentinel: Vec<u8>,
    pending: Vec<u8>,
}

impl SudoPromptFilter {
    pub(super) fn new(prompt_sentinel: String, started_sentinel: String) -> Self {
        Self {
            prompt_sentinel: prompt_sentinel.into_bytes(),
            started_sentinel: started_sentinel.into_bytes(),
            pending: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, chunk: Vec<u8>) -> Vec<SudoPromptAction> {
        self.pending.extend(chunk);
        let mut actions = Vec::new();
        while let Some((position, action)) = self.next_sentinel() {
            if position > 0 {
                actions.push(SudoPromptAction::Output(
                    self.pending.drain(..position).collect(),
                ));
            }
            let sentinel_len = match action {
                SudoPromptAction::Prompt => self.prompt_sentinel.len(),
                SudoPromptAction::Started => self.started_sentinel.len(),
                SudoPromptAction::Output(_) => unreachable!("sentinel action"),
            };
            self.pending.drain(..sentinel_len);
            actions.push(action);
        }

        let retained = self
            .prompt_sentinel
            .len()
            .max(self.started_sentinel.len())
            .saturating_sub(1);
        if self.pending.len() > retained {
            let emitted = self.pending.len() - retained;
            actions.push(SudoPromptAction::Output(
                self.pending.drain(..emitted).collect(),
            ));
        }
        actions
    }

    fn next_sentinel(&self) -> Option<(usize, SudoPromptAction)> {
        [
            (&self.prompt_sentinel, SudoPromptAction::Prompt),
            (&self.started_sentinel, SudoPromptAction::Started),
        ]
        .into_iter()
        .filter_map(|(sentinel, action)| {
            self.pending
                .windows(sentinel.len())
                .position(|window| window == sentinel)
                .map(|position| (position, action))
        })
        .min_by_key(|(position, _)| *position)
    }

    pub(super) fn finish(self) -> Option<Vec<u8>> {
        if self.pending.is_empty()
            || self.prompt_sentinel.starts_with(&self.pending)
            || self.started_sentinel.starts_with(&self.pending)
        {
            return None;
        }
        Some(self.pending)
    }
}
