# Privacy Policy

Last updated 2026-09-27

MIAQ turns a Discord message into a quote image. This page says what it sees and what it keeps. Short version: it keeps nothing.

## What MIAQ gets

Only when you use **Quote** or **/quote**, Discord sends MIAQ:

- the message you picked: its text, attachments and embeds
- the author's display name, username and avatar
- the channel, server and message IDs, and a short-lived token to reply with

For **/quote** with a link, MIAQ asks Discord for that one message itself, and only if it's in the channel you ran the command in.

## What it does with it

It downloads the author's avatar and the first image in the message, draws the quote image, and posts it as the reply to your command. All of that happens in memory during that one request.

## What it keeps

Nothing. MIAQ has no database and doesn't save messages, images, names or IDs. The quote image only exists in the Discord message it posts, which you can delete like any other message.

When you add MIAQ, Discord sends back a one-time code. MIAQ trades it with Discord to finish the install and throws the result away.

## Hosting

MIAQ runs on Cloudflare Workers. Cloudflare may keep normal request logs such as IP address and time, see the [Cloudflare Privacy Policy](https://www.cloudflare.com/privacypolicy/). MIAQ's own error messages can include technical details like image URLs. They are only visible live while debugging and aren't saved.

## Removing it

Remove MIAQ from User Settings, Authorized Apps, or from a server's Integrations settings. Since nothing is stored, there's nothing else to delete.

## Contact

Questions go to the [GitHub issues](https://github.com/sioworkers/make-it-a-quote/issues).
