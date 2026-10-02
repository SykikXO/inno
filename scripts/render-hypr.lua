-- Config for the isolated compositor that scripts/render-verify.sh starts.
--
-- Nothing here autostarts anything. That is the whole point: this compositor
-- exists so that a screenshot contains only what inno drew, and any rule that
-- puts a bar, a wallpaper or a launcher on screen defeats it.
--
-- It is Lua rather than the old .conf format for two reasons. Hyprland 0.57
-- removes .conf, so a .conf test config is already on borrowed time, and Hyprland
-- draws both deprecation notices and config errors as overlays across the
-- middle of the output. An overlay is indistinguishable from a notification
-- unless you already know it is there.
--
-- The script checks the surface is blank after startup and refuses to measure if
-- it is not, which catches a broken config here rather than in the numbers.

hl.config({
	misc = {
		-- The default wallpaper and the splash footer are Hyprland's own
		-- drawing. Static, so baseline differencing would tolerate them, but an
		-- empty surface makes a failure obvious instead of something to reason
		-- about.
		disable_splash_rendering = true,
		force_default_wallpaper = 0,
		disable_hyprland_logo = true,
		focus_on_activate = 0,
	},
})

-- No window animations. They would make the surface change over time for
-- reasons that have nothing to do with what is being measured.
hl.animation({ leaf = "workspaces", enabled = false })

-- grim must be allowed to copy the screen. Without this every capture blocks
-- until it times out.
hl.permission({ binary = "/usr/(local/)?(s)?bin/grim", type = "screencopy", mode = "allow" })