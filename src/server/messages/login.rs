use uuid::Uuid;

struct IdentClientParams {
    game_version: Uuid,
    language: String,
}

struct LoginClientParams {
    username: String,
    password: String,
}

enum LoginClientMessage {
    Ident(IdentClientParams),
    Login(LoginClientParams),
}

struct IdentServerParams {}

struct WelcomeServerParams {
    welcome_message: String,
}

struct RejectServerParams {
    reason: String,
}

enum LoginServerMessage {
    Ident(IdentServerParams),
    Welcome(WelcomeServerParams),
    Reject(RejectServerParams),
    Disconnect,
}
