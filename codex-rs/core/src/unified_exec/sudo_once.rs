pub(super) enum SudoPromptAction {
    Output(Vec<u8>),
    Prompt,
    Started,
}

#[derive(Default)]
pub(super) enum SudoAuthenticationState {
    #[default]
    AwaitingAuthentication,
    Started,
}

impl SudoAuthenticationState {
    pub(super) fn permits_credential_request(&self) -> bool {
        matches!(self, Self::AwaitingAuthentication)
    }

    pub(super) fn mark_started(&mut self) -> bool {
        if matches!(self, Self::AwaitingAuthentication) {
            *self = Self::Started;
            return true;
        }
        false
    }
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
        let withheld = [&self.prompt_sentinel, &self.started_sentinel]
            .into_iter()
            .filter_map(|sentinel| {
                (1..sentinel.len())
                    .rev()
                    .find(|&length| self.pending.ends_with(&sentinel[..length]))
            })
            .max()
            .unwrap_or_default();
        let output_len = self.pending.len().saturating_sub(withheld);
        (output_len > 0).then(|| self.pending[..output_len].to_vec())
    }
}
