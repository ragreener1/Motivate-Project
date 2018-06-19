import java.time.Instant

object Simulation {
  val totalYears = 1
  val numberOfPeople = 30000
  val numberOfSimulations = 1
  val socialConnectivity = 0.7f
  val subcultureConnectivity = 0.5f
  val neighbourConnectivity = 0.3f
  val numberOfSocialLinks = 20
  val numberOfNeighbourLinks = 20

  def main(args: Array[String]): Unit = {
    val t0 = Instant.now().getEpochSecond

    var threads: Set[Thread] = Set()

    for (i <- 1 to numberOfSimulations) {
      var thread = new Thread(new Borough(
        id = i,
        totalYears = totalYears,
        numberOfPeople = numberOfPeople,
        socialConnectivity = socialConnectivity,
        subcultureConnectivity = subcultureConnectivity,
        neighbourhoodConnectivity = neighbourConnectivity,
        numberOfSocialNetworkLinks = numberOfSocialLinks,
        numberOfNeighbourLinks = numberOfNeighbourLinks
      ))
      thread.start()
      threads += thread
    }

    threads.foreach(_.join())
    val t1 = Instant.now().getEpochSecond
    System.out.println(s"TOTAL RUNNING TIME: ${t1 - t0}s")
  }
}