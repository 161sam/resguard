use crate::cli::SuggestRequest;
use crate::*;
use resguard_services::suggest_service::{suggest, SuggestRequest as ServiceSuggestRequest};

pub(crate) fn handle_suggest(req: SuggestRequest) -> Result<i32> {
    let SuggestRequest {
        format,
        root,
        config_dir,
        state_dir,
        profile,
        apply,
        auto,
        dry_run,
        confidence_threshold,
    } = req;

    suggest(
        ServiceSuggestRequest {
            format,
            apply,
            auto,
            dry_run,
            confidence_threshold,
        },
        || resolve_suggest_profile(&root, &config_dir, &state_dir, profile.as_deref()),
        |desktop_id, class, force| {
            let code = handle_desktop_wrap(
                desktop_id,
                class,
                DesktopWrapOptions {
                    force,
                    dry_run: false,
                    print_only: false,
                    override_mode: false,
                },
            )?;
            if code == 0 {
                Ok(())
            } else {
                Err(anyhow!("wrap returned {code}"))
            }
        },
    )
}

pub(crate) fn run(req: SuggestRequest) -> Result<i32> {
    handle_suggest(req)
}
