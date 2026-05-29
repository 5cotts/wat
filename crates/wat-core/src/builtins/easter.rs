use crate::io::{ShellIo, SideEffect};

/// `help` — custom welcome message
pub fn help_easter(io: &mut ShellIo) -> i32 {
    io.write_out(
        "Welcome to wat, a shell that runs in your browser.\n\
         \n\
         Common commands:\n\
           ls          list files\n\
           ls -a       list all files (including hidden ones)\n\
           cat <file>  show file contents\n\
           cd <dir>    change directory\n\
           echo <msg>  print a message\n\
           clear       clear the screen\n\
           help        show this message\n\
         \n\
         Hint: try `ls -a` to see what's hiding.\n",
    );
    0
}

/// `sudo <anything>` — joke refusal
pub fn sudo(io: &mut ShellIo) -> i32 {
    io.write_out(
        "Sorry, this incident will be reported.\n\
         (Just kidding. But also: no.)\n",
    );
    1
}

/// `vim`, `vi`, `nano`, `emacs` — editor trap
pub fn editor_trap(name: &str, io: &mut ShellIo) -> i32 {
    match name {
        "vim" | "vi" => io.write_out(
            "You have entered vim. To exit, press Esc, then type ':q!' and press Enter.\n\
             Just kidding — this shell doesn't have vim. You're safe.\n",
        ),
        "emacs" => io.write_out(
            "Starting emacs... M-x butterfly... M-x doctor... C-x C-c to quit...\n\
             Actually this shell doesn't have emacs either. Take a breath.\n",
        ),
        _ => io.write_out("No editor here. Just the void and a blinking cursor.\n"),
    }
    1
}

/// `sl` — steam locomotive ASCII art
pub fn sl(io: &mut ShellIo) -> i32 {
    io.write_out(
        "\r\n\
         \x1b[33m      ====        ________                ___________\n\
         _D _|  |_______/        \\__I_I_____===__|_________|\n\
          |(_)---  |   H\\________/ |   |        =|___ ___|      _________________\n\
          /     |  |   H  |  |     |   |         ||_| |_||     _|                \\_____A\n\
         |      |  |   H  |__--------------------| [___] |   =|                        |\n\
         | ________|___H__/__|_____/[][]~\\_________|       |   -|                        |\n\
         |/ |   |-----------I_____I [][] []  D   |=======|____|________________________|_\n\
       __/ =| o |=-~~\\  /~~\\  /~~\\  /~~\\ ____Y___________|__|__________________________|_\n\
        |/-=|___|=    ||    ||    ||    |_____/~\\___/          |_D__D__D_|  |_D__D__D_|\n\
         \\_/      \\O=====O=====O=====O_/      \\_/               \\_/   \\_/    \\_/   \\_/\n\
         \x1b[0m\n\
         \x1b[31m   /\\_/\\   (    )\n\
          ( o.o )  / ----\n\
           > ^ <  /|\n\
          /|   |\\/ |\n\
         (_)   (_)_)\x1b[0m\n",
    );
    0
}

/// `./whoami.sh`, `bash whoami.sh`, `sh whoami.sh` — redirect to scottschmidt.io
pub fn whoami_sh(io: &mut ShellIo) -> i32 {
    io.write_out("i am scott\n");
    io.emit_side_effect(&SideEffect::Redirect {
        url: "https://www.scottschmidt.io".to_string(),
        delay_ms: Some(800),
    });
    0
}

/// `__konami__` — internal command triggered by the Konami code in the browser
pub fn konami(io: &mut ShellIo) -> i32 {
    io.write_out("\x1b[1;35m*** KONAMI CODE ***\x1b[0m\n");
    io.emit_side_effect(&SideEffect::KonamiCelebrate);
    0
}
