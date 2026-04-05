ALTER TABLE `users` ADD `profile_setup_completed` integer DEFAULT false NOT NULL;
UPDATE `users` SET `profile_setup_completed` = 1;
UPDATE `users` SET `profile_setup_completed` = 1;
