
use pipl::*;

const PF_PLUG_IN_VERSION: u16 = 13;
const PF_PLUG_IN_SUBVERS: u16 = 28;

#[rustfmt::skip]
fn main() {
    pipl::plugin_build(vec![
        Property::Kind(PIPLType::AEEffect),
        Property::Name("Golobulus"),
        Property::Category("Python"),

        #[cfg(target_os = "windows")]
        Property::CodeWin64X86("EffectMain"),
        #[cfg(target_os = "macos")]
        Property::CodeMacIntel64("EffectMain"),
        #[cfg(target_os = "macos")]
        Property::CodeMacARM64("EffectMain"),

        Property::AE_PiPL_Version { major: 2, minor: 0 },
        Property::AE_Effect_Spec_Version { major: PF_PLUG_IN_VERSION, minor: PF_PLUG_IN_SUBVERS },
        Property::AE_Effect_Version {
            version: 1,
            subversion: 0,
            bugversion: 0,
            stage: Stage::Develop,
            build: 0,
        },
        Property::AE_Effect_Info_Flags(0),
        Property::AE_Effect_Global_OutFlags(
            OutFlags::IDoDialog
            | OutFlags::PixIndependent
            | OutFlags::DeepColorAware
            | OutFlags::SendUpdateParamsUI
            | OutFlags::NonParamVary,
        ),
        Property::AE_Effect_Global_OutFlags_2(
            OutFlags2::FloatColorAware
            | OutFlags2::SupportsSmartRender
            | OutFlags2::SupportsThreadedRendering
            | OutFlags2::SupportsGetFlattenedSequenceData
            | OutFlags2::ParamGroupStartCollapsedFlag,
        ),
        Property::AE_Effect_Match_Name("Golobulus"),
        Property::AE_Reserved_Info(0),
        Property::AE_Effect_Support_URL("https://github.com/mobile-bungalow"),
    ]);

    if !cfg!(debug_assertions) && cfg!(target_os = "windows") {
        // Link a manifest to redirect dll search to the contained version
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED,ID=2");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:golob_plugin/pyo3_configs/Golobulus.aex.manifest");
    }

}
