ALTER TABLE `settings` ADD `default_echo_cancellation` integer DEFAULT true NOT NULL;--> statement-breakpoint
ALTER TABLE `settings` ADD `default_noise_suppression` text DEFAULT 'standard' NOT NULL;