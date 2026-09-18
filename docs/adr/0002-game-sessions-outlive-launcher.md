# Game sessions outlive the launcher

Closing or quitting GWNativeLauncher leaves running game instances alive, so launcher lifetime does not control the lifetime of a player's sessions. Reopening the launcher reconnects to those instances; stopping one game and quitting the launcher with all games are separate explicit actions. This trades simpler quit-with-all ownership for independent sessions and requires running-state reconciliation after launcher restart.
