import { responseFromJson } from "@chiselstrike/api"
import { BlogComment } from "../models/BlogComment"

type Handler = (req: Request, res: Response) => Response | Promise<Response>

const handlePost: Handler = async req => {
  const payload = await req.json()
  const created = BlogComment.build({
    ...payload,
    postedAt: new Date().toISOString(),
  })
  await created.save()
  return responseFromJson("inserted " + created.id)
}

const handleGet: Handler = async req => {
  const url = new URL(req.url)
  const postId = url.searchParams.get("postId") ?? undefined
  const comments = await BlogComment.findMany({ postId })
  return responseFromJson(comments)
}

const handlers: Record<string, Handler> = {
  POST: handlePost,
  GET: handleGet,
}

export default async function chisel(req: Request, res: Response) {
  if (handlers[req.method] === undefined)
    return new Response(`Unsupported method ${req.method}`, { status: 405 })
  return handlers[req.method](req, res)
}
