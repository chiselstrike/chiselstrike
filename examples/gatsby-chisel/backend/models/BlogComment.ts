import { ChiselEntity } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
  postId: string = ""
  content: string = ""
  postedAt: string = ""
}
