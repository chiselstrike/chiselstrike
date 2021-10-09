# Releasing a new version of chiseld

Although we plan to make this better in the future, right now the way to release
is to push anything to a branch called "release". This will create two docker containers:

* chiseld
* toolset (contains chisel)

and push them to an ECR registry. There are two tags pushed: one with the git hash of the commit that
produced the image, and another with the special tag "latest"
