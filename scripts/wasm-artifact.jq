[.[] | select(.reason == "compiler-artifact" and .target.name == $target)
     | .filenames[] | select(endswith(".wasm"))]
| if length == 1 then .[0] else error("expected one Wasm artifact for " + $target) end
