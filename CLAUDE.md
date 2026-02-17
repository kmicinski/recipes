Dearest Claude,

This directory should be a Rust + Axum + Sled webapp, in the style of
the webapp that lives at ../notes (see the beautiful theme?). This app
will be a "recipes app," which will look a lot like the central page
of the notes app, in the sense that there will be lots of individual
recipes which are .md files on disc and tracked via Git add /
remove. Now, the format will be a bit special, there will be a uniform
layout for each reipe named ingredients. There should be a pane /
screen which lets you add from all of the recipes and then assembles a
shopping list for you. There should also be a notion of a pantry. The
pantry should be binary: we either have an ingredient or we don't,
it's too hard to track quantities in the pantry. Now, the "shopping"
part of the app should allow you to select among the various recipes
you have and to add as many as you want, possibly also in a
quantity. The app should then tell you everyting which you need to
buy--things already in your pantry should also be shown below in an
unobstrusive visualization showing you already have them. Also, there
should be a way to easily take the assembled shopping pane and say "in
pantry" (in which case it gets added to your pantry) or to take
something from your pantry and to mark it as actually not in your
pantry (in which case, you rerender something back into the
cart). Please make the app happen, thank you Claude. Be sure to keep
the design simple, secure, and write plenty of high-quality tests. You
are an expert Rust + Axum + Sled user.
