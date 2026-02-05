use std::sync::{mpsc, Arc, RwLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LlmState {
    RunningInference,
    AwaitingInput,
    RunningTts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifeCycleState {
    Initializing,
    Running,
    ShuttingDown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LlmCommand {
    ContinueConversation(String),
    EditLastMessage(String),
    CancelInference,
    DestroyContextAndRunFromNothing(Vec<(String, String)>),
    DestroySystemPromptAndContinueConversation(String),
}

#[derive(Clone, Debug)]
pub struct State {
    pub life_cycle_state: LifeCycleState,
    pub conversation: Vec<(String, String)>,
    pub text_input: Option<(String, usize)>,
    pub system_mute: bool,
    pub user_mute: bool,
    pub is_editing: bool,
    pub llm_command: Option<LlmCommand>,
    pub llm_state: LlmState,
    pub tts_command: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            life_cycle_state: LifeCycleState::Initializing,
            conversation: Vec::new(),
            system_mute: true,
            user_mute: false,
            is_editing: false,
            text_input: None,
            llm_command: None,
            llm_state: LlmState::AwaitingInput,
            tts_command: None,
        }
    }
}

#[derive(Clone)]
pub struct StateHandle {
    state: Arc<RwLock<State>>,
    subscribers: Arc<RwLock<Vec<mpsc::Sender<()>>>>,
}

impl StateHandle {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(State::default())),
            subscribers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Read the current state
    pub fn read(&self) -> State {
        self.state.read().unwrap().clone()
    }

    /// Mutate the state and notify all subscribers
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut State),
    {
        {
            let mut state = self.state.write().unwrap();
            f(&mut state);
        }
        self.notify();
    }

    /// Subscribe to state changes, returns a receiver that gets notified on updates
    pub fn subscribe(&self) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        self.subscribers.write().unwrap().push(tx);
        rx
    }

    fn notify(&self) {
        let mut subs = self.subscribers.write().unwrap();
        // Remove disconnected subscribers
        subs.retain(|tx| tx.send(()).is_ok());
    }
}

impl Default for StateHandle {
    fn default() -> Self {
        Self::new()
    }
}
