use resguard_model::SuggestRule;

pub fn default_suggest_rules() -> Vec<SuggestRule> {
    vec![
        SuggestRule {
            pattern: "(?i)docker|podman|containerd".to_string(),
            class: "heavy".to_string(),
        },
        SuggestRule {
            pattern:
                "(?i)code|codium|vscodium|idea|pycharm|clion|goland|webstorm|rubymine|phpstorm|datagrip|rider|jetbrains"
                    .to_string(),
            class: "ide".to_string(),
        },
        SuggestRule {
            pattern:
                "(?i)firefox|chrome|chromium|chromium-browser|google-chrome|brave|opera|vivaldi"
                    .to_string(),
            class: "browsers".to_string(),
        },
    ]
}
