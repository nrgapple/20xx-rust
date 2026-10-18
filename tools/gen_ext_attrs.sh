#!/bin/sh
# Regenerate crates/melee/src/ft/ext_data.rs (per-character attribute layouts).
# Overrides fix decomp header types; evidence is in the generated file header.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
src="$root/reference/melee/src"
python3 "$root/tools/gen_dat_structs.py" "$src/melee" "$src/sysdolphin" -- \
  ftMario_DatAttrs=MarioAttributes ftFox_DatAttrs=FoxAttributes \
  ftCaptain_DatAttrs=CaptainAttributes ftDonkeyAttributes=DonkeyAttributes \
  ftKb_DatAttrs=KirbyAttributes ftKoopaAttributes=KoopaAttributes \
  ftLk_DatAttrs=LinkAttributes ftSeakAttributes=SeakAttributes \
  ftNessAttributes=NessAttributes ftPe_DatAttrs=PeachAttributes \
  ftIceClimberAttributes=IceClimberAttributes ftPikachuAttributes=PikachuAttributes \
  ftSs_DatAttrs=SamusAttributes ftYoshiAttributes=YoshiAttributes \
  ftPurinAttributes=PurinAttributes ftMewtwoAttributes=MewtwoAttributes \
  ftLuigiAttributes=LuigiAttributes MarsAttributes=MarsAttributes \
  ftZelda_DatAttrs=ZeldaAttributes ftGameWatchAttributes=GameWatchAttributes \
  ftPurinAttributes.x0:int ftPurinAttributes.x14:float ftPurinAttributes.x1c:float \
  ftPurinAttributes.x20:float ftPurinAttributes.x24:float \
  ftCaptain_DatAttrs.x68:int ftIceClimberAttributes.x1c:float \
  ftZelda_DatAttrs.x28:float ftZelda_DatAttrs.x30:float "$@"
