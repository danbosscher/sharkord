This repository is a fork of the original [Sharkord/sharkord](https://github.com/Sharkord/sharkord) project.
For what changed in this fork, see [`todo.md`](todo.md). That file tracks the fork-specific backlog, what was implemented locally, what was intentionally deferred, and the main operational decisions made for this small private deployment.

## Getting Started

This fork only supports running Sharkord as a headless server in Docker. The browser client is served by that server container.

#### Docker

```bash
docker run \
  -p 4991:4991/tcp \
  -p 40000:40000/tcp \
  -p 40000:40000/udp \
  -v ./data:/home/bun/.config/sharkord \
  --name sharkord \
  ghcr.io/<owner>/sharkord:latest
```

> [!NOTE]
> Upon first launch, Sharkord will create a secure token and print it to the console. This token allows ANYONE to gain owner access to your server, so make sure to store it securely and do not lose it!

Once the server is running, open your web browser and navigate to [http://localhost:4991](http://localhost:4991) to access the Sharkord client interface. If you're running the server on a different machine, replace `localhost` with the server's IP address or domain name.

## Private Join Shortcut

For this fork's small trusted-group deployment, the browser client also accepts a temporary server-password shortcut in the URL:

```text
https://chat.example.org/?serverpassword=example
```

After account login, Sharkord will try that password automatically for the server join step and then remove `serverpassword` from the address bar.

> [!WARNING]
> This is intentionally low-friction, not high-security. Query parameters leak into browser history, screenshots, copied links, and sometimes proxy or analytics logs. Use this only for a tiny private server where everyone already has the shared password anyway. If this deployment grows beyond that, replace it with a signed invite token flow instead of shipping the raw server password in the URL.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Built with amazing open-source technologies:

- [Bun](https://bun.sh)
- [tRPC](https://trpc.io)
- [Mediasoup](https://mediasoup.org)
- [Drizzle ORM](https://orm.drizzle.team)
- [React](https://react.dev)
- [Radix UI](https://www.radix-ui.com)
- [ShadCN UI](https://ui.shadcn.com/)
- [Tailwind CSS](https://tailwindcss.com)

<div align="center">
  <p>Made with ❤️ by the Sharkord team</p>
  <p>
    <a href="https://github.com/Sharkord/sharkord">GitHub</a> •
    <a href="https://github.com/Sharkord/sharkord/issues">Issues</a> •
    <a href="https://github.com/Sharkord/sharkord/discussions">Discussions</a>
  </p>
</div>
